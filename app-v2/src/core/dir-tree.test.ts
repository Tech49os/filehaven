import { describe, expect, it } from 'vitest'
import { collapsedBeyondDepth } from './dir-tree'
import type { DirNode } from './types'

function node(
  id: number,
  depth: number,
  children: DirNode[] = [],
): DirNode {
  return {
    id,
    name: `目录${id}`,
    relPath: `目录${id}`,
    depth,
    origin: 'user',
    fileCount: 0,
    children,
  }
}

describe('collapsedBeyondDepth（BUG-002 默认折叠策略）', () => {
  // 3 层样例树：第 3 层有子节点（应进集合），其下的第 4 层叶子不进集合
  const tree = [
    node(1, 1, [
      node(2, 2, [
        node(3, 3, [node(4, 4)]), // 第 3 层有子节点 → 折叠它
        node(5, 3), // 第 3 层叶子 → 不进集合
      ]),
    ]),
    node(6, 1), // 第 1 层叶子
  ]

  it('默认 depth=2：递归收集 depth>=3 且有子节点的目录 id', () => {
    expect([...collapsedBeyondDepth(tree)].sort((a, b) => a - b)).toEqual([3])
  })

  it('叶子节点不进集合（第 3 层叶子、第 4 层叶子都不收）', () => {
    const out = collapsedBeyondDepth(tree)
    expect(out.has(5)).toBe(false)
    expect(out.has(4)).toBe(false)
    expect(out.has(6)).toBe(false)
  })

  it('第 1、2 层即使有子节点也不折叠', () => {
    const out = collapsedBeyondDepth(tree)
    expect(out.has(1)).toBe(false)
    expect(out.has(2)).toBe(false)
  })

  it('depth=1 边界：第 2 层起有子节点的目录进集合，第 3 层同样收', () => {
    const out = collapsedBeyondDepth(tree, 1)
    expect([...out].sort((a, b) => a - b)).toEqual([2, 3])
  })

  it('只有 1 层时返回空集合，不报错', () => {
    expect(collapsedBeyondDepth([node(1, 1), node(2, 1)]).size).toBe(0)
  })

  it('空树返回空集合', () => {
    expect(collapsedBeyondDepth([]).size).toBe(0)
  })
})
