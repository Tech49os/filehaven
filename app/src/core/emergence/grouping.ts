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

export interface TopicGroup {
  name: string
  aliases: string[]
  /** 组内所有候选的 df 之和，用于判断这组有多少文件 */
  weight: number
}

/** 长度差超过这个值就不认为是同一件事的简写/全称 */
const MAX_LEN_GAP = 8

function related(a: string, b: string): boolean {
  if (a === b) return true
  if (Math.abs(a.length - b.length) > MAX_LEN_GAP) return false
  return a.includes(b) || b.includes(a)
}

export function groupBySurface(cands: Candidate[]): TopicGroup[] {
  // 先放长词：组名直接取最具体的那个写法
  const sorted = [...cands].filter((c) => !c.isOrg).sort((a, b) => b.term.length - a.term.length)
  const groups: { name: string; aliases: string[]; weight: number }[] = []

  for (const c of sorted) {
    const hit = groups.find((g) => g.aliases.some((a) => related(a, c.term)))
    if (hit) {
      hit.aliases.push(c.term)
      hit.weight += c.df
    } else {
      groups.push({ name: c.term, aliases: [c.term], weight: c.df })
    }
  }

  groups.sort((a, b) => b.weight - a.weight)
  return groups
}
