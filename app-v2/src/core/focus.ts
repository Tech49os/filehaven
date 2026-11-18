/**
 * 焦点推进与撤销栈（详设 §8.3）
 *
 * 纯函数，不碰 DOM / 不碰 store：单测可以直接跑。
 */

import type { BatchItem } from './types'

/** 已确认的判定（暂缓、先放着都还没定，仍算未确认） */
function isDone(it: BatchItem): boolean {
  return it.file.status === 'confirmed'
}

/**
 * 返回 `from` 之后（含 `from`）第一个未确认项的下标；全部确认返回 -1。
 * 已确认项跳过，所以连续按回车会一路往下走。
 */
export function nextUnconfirmed(items: BatchItem[], from: number): number {
  if (items.length === 0) return -1
  const start = Math.max(0, Math.min(from, items.length - 1))
  for (let i = start; i < items.length; i += 1) {
    if (!isDone(items[i])) return i
  }
  // 后面全确认完了，从头再找一遍（用户可能跳着改）
  for (let i = 0; i < start; i += 1) {
    if (!isDone(items[i])) return i
  }
  return -1
}

/** 最近一次确认的 fileId 压栈；同一份重复压只留一条 */
export function pushUndo(stack: number[], fileId: number): number[] {
  if (stack[stack.length - 1] === fileId) return stack
  return [...stack, fileId]
}

/** 弹栈：返回栈顶 fileId 与剩下的栈；空栈返回 null 且栈不动 */
export function popUndo(stack: number[]): { fileId: number | null; rest: number[] } {
  if (stack.length === 0) return { fileId: null, rest: [] }
  const rest = stack.slice(0, -1)
  return { fileId: stack[stack.length - 1], rest }
}
