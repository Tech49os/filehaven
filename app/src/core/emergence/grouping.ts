/**
 * 保守分组：把候选词按「表面形态」归到一起
 *
 * 为什么不用模型做主路径（实测结论，写在这里防止以后又被改回去）：
 *  - qwen2.5:7b 面对 40 个候选时在两个极端之间摆动：说「合并」→ 压成 5 个
 *    笼统大类并把钻采院、差旅费全丢进噪音；说「别标 null」→ 38 个词各自成组，
 *    连「演讲稿」「ppt」都当事项。判断力不足以稳定完成这个任务。
 *  - qwen3.5:9b 在 8GB 显存上处理 5000 字 prompt 直接失败（199s 后连接断开）。
 *
 * 所以主路径改成纯字符串规则的保守分组：只把「明显是同一件事的两种写法」
 * 合到一起（包含关系 + 长度接近），合不动的就各自成组，交给用户在确认页合并。
 * 这样每一步都可解释、可复现、不依赖模型。
 */
import type { Candidate } from './candidates'
import { isOrgName } from './lexicon'

export interface TopicGroup {
  name: string
  aliases: string[]
  /** 组内所有候选的 df 之和，用于判断这组有多少文件 */
  weight: number
  /** 兜底类：提案阶段只捡其他事项不要的文件 */
  weak: boolean
  /**
   * 为什么降级：
   *  doc —— 整组都是文种词（演讲稿/操作手册）
   *  who —— 组名是人名/组织名（说不出「哪件事」，但对用户仍有信息量，
   *         所以不删掉，折叠起来让他自己决定）
   */
  weakReason?: 'doc' | 'who'
}

/** 长度差超过这个值就不认为是同一件事的简写/全称 */
const MAX_LEN_GAP = 8

function related(a: string, b: string): boolean {
  if (a === b) return true
  if (Math.abs(a.length - b.length) > MAX_LEN_GAP) return false
  return a.includes(b) || b.includes(a)
}

export interface GroupOptions {
  /**
   * 已判定为「人名/组织名」的词（见 whoJudge）。
   * 它们说不出「哪件事」，不参与分组——否则「海油发展」会把技术谱系的文件全吞掉。
   */
  who?: Set<string>
}

export function groupBySurface(cands: Candidate[], opts: GroupOptions = {}): TopicGroup[] {
  const who = opts.who ?? new Set<string>()
  // 先放长词：组名直接取最具体的那个写法
  // 注意：组织名不过滤掉——它们要作为「兜底类」留下（见下面的 weakReason='who'），
  // 直接删会让它们名下的文件被邻近事项吸走或掉进未归类，用户就看不到这批文件了
  const sorted = [...cands].sort((a, b) => b.term.length - a.term.length)
  const groups: TopicGroup[] = []
  const weakCount = new Map<TopicGroup, number>()

  for (const c of sorted) {
    const hit = groups.find((g) => g.aliases.some((a) => related(a, c.term)))
    if (hit) {
      hit.aliases.push(c.term)
      hit.weight += c.df
      if (c.weak) weakCount.set(hit, (weakCount.get(hit) ?? 0) + 1)
    } else {
      const g: TopicGroup = { name: c.term, aliases: [c.term], weight: c.df, weak: false }
      groups.push(g)
      if (c.weak) weakCount.set(g, 1)
    }
  }

  // 降级规则：
  //  · 组名本身是人名/组织名 → 兜底（它回答的是「谁」）
  //  · 否则，整组都是文种词才算兜底；混进一个真专名就是正经事项
  for (const g of groups) {
    if (who.has(g.name) || isOrgName(g.name)) {
      g.weak = true
      g.weakReason = 'who'
    } else if ((weakCount.get(g) ?? 0) === g.aliases.length) {
      g.weak = true
      g.weakReason = 'doc'
    }
  }

  groups.sort((a, b) => b.weight - a.weight)
  return groups
}
