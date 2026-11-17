/**
 * 候选专名抽取（涌现的第一步）
 *
 * 做的事：从文件名里找出「跨多份文件反复出现、但又不是公文套话」的词组，
 * 它们就是「一件持续在办的事」的候选名。
 *
 * 不做的事：判断哪个候选才是真事项。这一步交给模型（合并/命名/剔除组织名），
 * 因为算法在多份真实文件上实测清不干净——`智能工厂技术室` 和 `技术谱系`
 * 用纯统计手段区分不出来，但人一眼就知道。
 */
import { segments } from './normalize'
import { DOC_KIND_WORDS, GENERIC_WORDS, isOrgName } from './lexicon'

export interface FileRef {
  name: string
  /** 原始相对路径，目前只用于让模型看清上下文 */
  path: string
}

export interface Candidate {
  term: string
  /** 出现在多少份不同文件里 */
  df: number
  isOrg: boolean
  /** 送去给模型看的原始文件名，最多 3 个 */
  samples: string[]
  score: number
}

export interface CandidateOptions {
  /** 至少出现在几份文件里才算候选 */
  minDf?: number
  /** 出现在超过这个比例的文件里 → 是部门名/口头禅，丢弃 */
  maxDfRatio?: number
  /** 截断 */
  topK?: number
}

const STOP = new Set<string>([...DOC_KIND_WORDS, ...GENERIC_WORDS])

/** 整个词组是否由停用词拼成（如「方案材料」「工作情况报告」） */
function isAllStopWords(term: string): boolean {
  let i = 0
  while (i < term.length) {
    let matched = 0
    for (const w of STOP) {
      if (w.length > matched && term.startsWith(w, i)) matched = w.length
    }
    if (matched === 0) return false
    i += matched
  }
  return true
}

/**
 * 一个段能贡献的候选形态。
 *
 * 整段**不限制长度**：像「中海油信息科技有限公司科技技术谱系」这种 18 字长串
 * 本身不适合当类目名，但它是判断「中海油信息科技有」「海油信息科技有限」
 * 这类滑动窗口碎片的唯一依据——不收集它，碎片就永远找不到包含者。
 * 超长整段只在碎片判定里用，最后不会出现在候选输出里。
 */
function forms(seg: string): string[] {
  const chars = [...seg]
  const L = chars.length
  const out: string[] = []
  if (L >= 2) out.push(seg)
  // 段本身很短时不要再切，否则只会切出碎片
  if (L > 4) {
    for (let n = 3; n <= Math.min(8, L - 1); n++) {
      for (let i = 0; i + n <= L; i++) out.push(chars.slice(i, i + n).join(''))
    }
  }
  return out
}

/** 位置分：出现在文件名开头最可能是事项，结尾常是落款 */
function positionBonus(i: number, segCount: number): number {
  if (i === 0) return 1
  if (i === segCount - 1 && segCount > 1) return 0.5
  return 0.7
}

interface Stat {
  df: number
  samples: string[]
  bestPos: number
}

export function collectCandidates(files: FileRef[], opts: CandidateOptions = {}): Candidate[] {
  const minDf = opts.minDf ?? 2
  const maxDfRatio = opts.maxDfRatio ?? 0.08
  const topK = opts.topK ?? 80
  // 8% 上限只在文件量够大时才成立：样本里只有 4 份文件、全部提到同一件事，
  // 这是完全正常的（本来就只办了一件大事），不能被当成「部门口头禅」丢掉。
  // 所以 N < 10 时不做上限过滤，且下限固定为 3。
  const maxDf =
    files.length < 10 ? files.length : Math.max(3, Math.ceil(files.length * maxDfRatio))

  const stats = new Map<string, Stat>()
  /** 曾经作为完整段出现过的词（区别于从长段里切出来的子串碎片） */
  const wholeTerms = new Set<string>()

  for (const f of files) {
    const segs = segments(f.name)
    if (segs.length === 0) continue
    // 同一份文件里同一个词只算一次 df
    const seen = new Map<string, number>()
    for (let i = 0; i < segs.length; i++) {
      const bonus = positionBonus(i, segs.length)
      wholeTerms.add(segs[i])
      for (const term of forms(segs[i])) {
        const prev = seen.get(term)
        if (prev !== undefined) {
          seen.set(term, Math.max(prev, bonus))
        } else {
          seen.set(term, bonus)
        }
      }
    }
    for (const [term, bonus] of seen) {
      const s = stats.get(term)
      if (s) {
        s.df++
        if (bonus > s.bestPos) s.bestPos = bonus
        if (s.samples.length < 3) s.samples.push(f.name)
      } else {
        stats.set(term, { df: 1, samples: [f.name], bestPos: bonus })
      }
    }
  }

  // 先做便宜的过滤，再对幸存者做停用词判断
  const survivors: { term: string; st: Stat }[] = []
  for (const [term, st] of stats) {
    if (st.df < minDf || st.df > maxDf) continue
    survivors.push({ term, st })
  }

  const kept = survivors.filter(({ term }) => !STOP.has(term) && !isAllStopWords(term))
  const keptTerms = kept.map((k) => k.term)

  // ── 邻接统计（第二遍扫描）─────────────────────────────────
  //
  // 光靠 df 抓不住「跨多个父词」的碎片：`化建设` 同时来自「数字化建设」和
  // 「智能化建设」，没有任何单个长词的出现次数能压过它。
  //
  // 真正的碎片有两个可测特征：
  //  1. 从不在段的开头或结尾出现 → 它必然是某段话的中间片段
  //  2. 左（或右）邻字永远只有一种，且加上它之后也是候选 → 它是那个词的尾巴/头
  // 真事项反过来：一定在段首/段尾出现过，且邻字多样
  // （`技术谱系` 左边可以是「智能感知」「软件开发」或段首，右边可以是 -、汇报）。
  const wanted = new Set(keptTerms)
  interface Adj {
    lefts: Set<string>
    rights: Set<string>
    atStart: boolean
    atEnd: boolean
  }
  const adj = new Map<string, Adj>()
  for (const f of files) {
    for (const seg of segments(f.name)) {
      const chars = [...seg]
      for (let i = 0; i < chars.length; i++) {
        for (let n = 3; n <= 8 && i + n <= chars.length; n++) {
          const t = chars.slice(i, i + n).join('')
          if (!wanted.has(t)) continue
          let a = adj.get(t)
          if (!a) {
            a = { lefts: new Set(), rights: new Set(), atStart: false, atEnd: false }
            adj.set(t, a)
          }
          if (i === 0) a.atStart = true
          else if (a.lefts.size < 8) a.lefts.add(chars[i - 1])
          if (i + n === chars.length) a.atEnd = true
          else if (a.rights.size < 8) a.rights.add(chars[i + n])
        }
      }
    }
  }

  // 碎片剔除（真实 450 份数据暴露的坑）
  //
  // 长段会产出大量互相重叠、但**互不为子串**的定长窗口，例如
  // 「中海油信息科技有限公司科技技术谱系」会切出：
  //   中海油信息科技有 / 海油信息科技有限 / 油信息科技有限公 ……
  // 它们长度相同、df 相同，按「包含关系」完全判不出来。
  //
  // 但这些碎片有一个共同特征：**总有一个更长的候选包含它，且出现次数几乎一样多**
  // （说明它从来没有独立出现过）。所以判据是：
  //   A ⊂ B 且 df(B) ≥ df(A) × 0.8  →  A 只是 B 的一部分，丢掉 A
  //
  // 反例保护：`技术谱系`(18) 虽被 `中海油信息科技有限公司科技技术谱系`(8) 包含，
  // 但 8 < 18×0.8，说明它在别的上下文里独立出现过 10 次——它是真词，必须留。
  //
  // 实现从「长词」出发反向标记：枚举长词的所有子串，命中候选就判定为碎片。
  // 比「每个短词去搜更长词」快得多（后者在 450 份真实文件上会退化成 O(n²)）。
  const dropped = new Set<string>()
  for (const t of keptTerms) {
    const a = adj.get(t)
    if (!a) continue
    // 1) 从未出现在段的边界上 → 纯中间片段
    if (!a.atStart && !a.atEnd) {
      dropped.add(t)
      continue
    }
    // 2) 左邻唯一，且左邻+自己也是候选 → 自己只是那个词的尾部
    if (a.lefts.size === 1) {
      const l = [...a.lefts][0]
      if (wanted.has(l + t)) {
        dropped.add(t)
        continue
      }
    }
    // 3) 右邻唯一，且自己+右邻也是候选 → 自己只是那个词的开头
    if (a.rights.size === 1) {
      const r = [...a.rights][0]
      if (wanted.has(t + r)) {
        dropped.add(t)
      }
    }
  }

  const byLenDesc = [...keptTerms].sort((a, b) => b.length - a.length)
  for (const b of byLenDesc) {
    if (b.length < 4) continue
    const db = stats.get(b)!.df
    for (let n = 3; n < b.length; n++) {
      for (let i = 0; i + n <= b.length; i++) {
        const a = b.slice(i, i + n)
        if (dropped.has(a)) continue
        const st = stats.get(a)
        if (!st) continue
        if (db >= st.df * 0.8) dropped.add(a)
      }
    }
  }

  const result: Candidate[] = []
  for (const { term, st } of kept) {
    if (dropped.has(term)) continue
    // 超过 12 字的整段只用来判碎片，当类目名太长（用户没法用）
    if (term.length > 12) continue
    const isOrg = isOrgName(term)
    result.push({
      term,
      df: st.df,
      isOrg,
      samples: st.samples,
      score: st.df * (isOrg ? 0.4 : 1) * st.bestPos,
    })
  }

  result.sort((a, b) => b.score - a.score || b.df - a.df || b.term.length - a.term.length)
  return result.slice(0, topK)
}
