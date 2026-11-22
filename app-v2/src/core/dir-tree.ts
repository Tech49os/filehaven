/**
 * 目录树默认折叠策略（BUG-002）。
 *
 * DirNode.depth 从 1 起。depth=2（默认）时，第 1、2 层保持展开，
 * 递归收集 depth >= 3 且**有子节点**的目录 id —— 折叠它们后，界面
 * 只展开到第 2 层，更深的节点不渲染。
 *
 * 叶子不进集合：没有子节点的目录折叠与否没有视觉差异，收进来只会
 * 让 Set 无意义地变大。两处页面（浏览页 / 设置页）共用此函数，
 * 避免默认策略在两处漂移（BUG-010）。
 */
import type { DirNode } from './types'

export function collapsedBeyondDepth(nodes: DirNode[], depth = 2): Set<number> {
  const out = new Set<number>()
  const walk = (list: DirNode[]) => {
    for (const n of list) {
      if (n.depth > depth && n.children.length > 0) out.add(n.id)
      walk(n.children)
    }
  }
  walk(nodes)
  return out
}
