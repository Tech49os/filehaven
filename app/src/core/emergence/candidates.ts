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

/** 一个段能贡献的候选形态：整段 + 段内 3–8 字 n-gram */
function forms(seg: string): string[] {
  const chars = [...seg]
  const L = chars.length
  const out: string[] = []
  if (L >= 2 && L <= 12) out.push(seg)
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

  // 同族去重（原型踩过的坑）：
  // 一个长词会同时产出它的所有子串，这些子串的 df 与长词完全相同。
  // 例如「智能工厂技术室」会带出「智能工」「智能工厂」「工厂技术室」……
  // 若按「短的代表优先」，反而会把完整的词删掉、留下一堆碎片。
  // 正确做法：同 df 的包含族里只留**最完整的一个**——
  //   whole=1（曾经作为整段出现）优先，其次取最长。
  const keptTerms = kept.map((k) => k.term)
  const wholeSet = wholeTerms
  const byDf = new Map<number, string[]>()
  for (const t of keptTerms) {
    const df = stats.get(t)!.df
    const arr = byDf.get(df)
    if (arr) arr.push(t)
    else byDf.set(df, [t])
  }
  const dropped = new Set<string>()
  for (const terms of byDf.values()) {
    const sorted = [...terms].sort((a, b) => {
      const wa = wholeSet.has(a) ? 1 : 0
      const wb = wholeSet.has(b) ? 1 : 0
      return wb - wa || b.length - a.length
    })
    const reps: string[] = []
    for (const t of sorted) {
      if (reps.some((r) => r.includes(t))) dropped.add(t)
      else reps.push(t)
    }
  }

  const result: Candidate[] = []
  for (const { term, st } of kept) {
    if (dropped.has(term)) continue
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
