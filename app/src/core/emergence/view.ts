/**
 * 确认页的展示规则（纯函数，与 Vue 无关）。
 *
 * 为什么份数要分档：分类不折叠时，31 行长得一模一样，眼睛没有落点。
 * 把份数做成左侧的大号数字并分三档着色，扫视时先看到「量」、再看「名」，
 * 主次是视觉上分出来的，不是靠隐藏小类分出来的。
 */

/** 份数档位：major ≥10 / normal 4-9 / minor ≤3 */
export type CountTier = 'major' | 'normal' | 'minor'

export function countTier(n: number): CountTier {
  if (n >= 10) return 'major'
  if (n >= 4) return 'normal'
  return 'minor'
}

/**
 * 按份数降序排，份数相同保持原顺序。
 *
 * 为什么要排：reassign 会重算归属但不重排顺序，改名/合并之后可能出现
 * 「2 份的排在 8 份前面」，主次又乱了。排序在展示层做，不动数据。
 */
export function sortForDisplay<T extends { fileIds: readonly unknown[] }>(
  topics: readonly T[]
): T[] {
  return topics
    .map((t, i) => ({ t, i }))
    .sort((a, b) => b.t.fileIds.length - a.t.fileIds.length || a.i - b.i)
    .map((x) => x.t)
}
