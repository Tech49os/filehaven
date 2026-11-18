/**
 * 组织/人名判定（唯一交给模型做的一步）
 *
 * 这条分工是实测出来的，不是拍的。同一批 57 个真实候选、两个模型、各 30-40 秒：
 *
 *  · 让模型「归并并命名」→ 4b 直接把组名写成「部门/公司/组织名」「放之四海皆准的空话」，
 *    它把我的判据当成组名了（30s，不可用）
 *  · 让它判 matter/org/person/doc/generic 五类 → 组织/人名很准，
 *    但「空话 vs 真事项」崩了：9b 把技术谱系/境内差旅费/安环大屏塞进 generic，
 *    4b 全塞进 doc。这类边界它俩都没有判断力，而字符串规则（DOC_KIND_WORDS +
 *    包含关系）在这件事上本来就更强
 *  · 只问「是不是人名/组织名」→ 9b：组织/人名 8/10、误伤 0/21、57 条全答、
 *    0 编造，33.6 秒。这才是模型真正擅长的那一件小事
 *
 * 所以最终分工：模型只回答「这个词是不是某个具体的人/单位」，
 * 剩下的（文种、空话、别名合并）全部留给规则。
 * 模型挂了就降级到 lexicon.isOrgName，功能不消失，只是漏几个。
 */
import type { Candidate } from './candidates'
import { isOrgName } from './lexicon'

export type WhoKind = 'org' | 'person' | 'no'

const VALID: readonly string[] = ['org', 'person', 'no']

/** 模型爱在英文/数字和汉字之间插空格（`AI 编程`、`作业 H5 培训游戏`），比对前先抹掉 */
export function normKey(s: string): string {
  return s.replace(/\s+/g, '')
}

export function buildWhoPrompt(cands: Candidate[]): string {
  const lines = cands.map((c, i) => {
    const sample = (c.samples[0] ?? '').slice(0, 24)
    return `${i + 1}. ${c.term} · ${c.df}份 · ${sample}`
  })
  return `下面是从一批文件名里抽出的高频词组，每行是「词组 · 出现份数 · 一个代表文件名」。

只回答一个问题：**这个词组是人名或组织名吗？**

- person：某个人的姓名
- org：部门、公司、单位、团队、项目组的名称（回答「谁在办」）
- no：都不是（回答「哪件事」「什么东西」）

注意：带地名的项目名不是组织名（「A厂设备改造」是事，「A厂技术室」才是组织）；
职位/角色不是人名（「全栈工程师」不指某个具体的人）。

候选词组：
${lines.join('\n')}

每个词组都要出现一次，不许漏，不许编造。
只返回 JSON：{"items":[{"term":"词组","kind":"org"}]}`
}

/**
 * 解析模型输出。
 * 只保留「候选池里确实有」且「kind 合法」的条目——模型编造的词和不在集合内的
 * kind 一律丢弃，不给下游制造幻觉。
 */
export function parseWhoResult(raw: string, cands: Candidate[]): Map<string, WhoKind> {
  const out = new Map<string, WhoKind>()
  const m = raw.match(/\{[\s\S]*\}/)
  if (!m) return out
  let obj: { items?: unknown }
  try {
    obj = JSON.parse(m[0])
  } catch {
    return out
  }
  const pool = new Map(cands.map((c) => [normKey(c.term), c.term]))
  for (const item of Array.isArray(obj.items) ? obj.items : []) {
    if (!item || typeof item !== 'object') continue
    const { term, kind } = item as { term?: unknown; kind?: unknown }
    const real = pool.get(normKey(String(term ?? '').trim()))
    const k = String(kind ?? '').trim().toLowerCase()
    if (!real || !VALID.includes(k)) continue
    out.set(real, k as WhoKind)
  }
  return out
}

/** 前缀扩散时要求的基础词长度：2 字的「集团」扩散出去会误伤「集团化运作」 */
const MIN_PREFIX_LEN = 3

/**
 * 把「后缀词典」这条老规则的判断并进来。
 *
 * 模型会漏（7b 漏了「生产智能化项目组」，9b 漏了「海油发展数字化」），
 * 但「XX项目组 / XX技术室 / XX公司」这类后缀是铁的，规则比模型稳。
 * 两边取并集，再叠加前缀扩散。
 */
export function augmentWithOrgRule(
  base: Map<string, WhoKind>,
  cands: Candidate[]
): Map<string, WhoKind> {
  const out = new Map(base)
  for (const c of cands) {
    if (!out.has(c.term) && isOrgName(c.term)) out.set(c.term, 'org')
  }
  return out
}

/**
 * 组织名开头的复合词也算组织名。
 *
 * 模型漏了「海油发展数字化」「海油发展智能工厂」这两个（它们读起来像项目名），
 * 但它们恰恰是吞文件最多的大桶。既然「海油发展」已被判为组织名，
 * 以它开头的更长的词也不可能是「一件事」——这条规则把模型漏的补上，
 * 而且不需要再调一次模型。
 */
export function expandWho(base: Map<string, WhoKind>, cands: Candidate[]): Set<string> {
  const who = new Set<string>()
  for (const [term, k] of base) if (k === 'org' || k === 'person') who.add(term)

  const bases = [...who].filter((t) => t.length >= MIN_PREFIX_LEN)
  if (bases.length === 0) return who
  for (const c of cands) {
    if (who.has(c.term)) continue
    if (bases.some((b) => c.term.length > b.length && c.term.startsWith(b))) who.add(c.term)
  }
  return who
}
