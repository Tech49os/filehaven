import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import DirTree from './DirTree.vue'
import type { DirNode } from '../core/types'

function node(
  id: number,
  name: string,
  relPath: string,
  depth: number,
  children: DirNode[] = [],
): DirNode {
  return { id, name, relPath, depth, origin: 'user', fileCount: 0, children }
}

const TREE = [
  node(1, '合同', '合同', 1, [node(2, '采购', '合同/采购', 2)]),
  node(3, '安全检查', '安全检查', 1),
]

describe('DirTree（T6.8）', () => {
  it('递归渲染两层，父节点带文件数', () => {
    const w = mount(DirTree, { props: { nodes: TREE, selectedId: null } })
    const names = w.findAll('.name').map((n) => n.text())
    expect(names).toEqual(['合同', '采购', '安全检查'])
  })

  it('点击行抛 select，选中态跟着 selectedId', async () => {
    const w = mount(DirTree, { props: { nodes: TREE, selectedId: 3 } })
    expect(w.findAll('.row')[2].classes()).toContain('on')
    await w.findAll('.row')[1].trigger('click')
    expect(w.emitted('select')?.[0]).toEqual([2])
  })

  it('点折叠箭头抛 toggle，且不触发 select', async () => {
    const w = mount(DirTree, { props: { nodes: TREE, selectedId: null } })
    await w.find('.caret').trigger('click')
    expect(w.emitted('toggle')?.[0]).toEqual([1])
    expect(w.emitted('select')).toBeUndefined()
  })

  it('collapsed 里的节点不渲染子层', () => {
    const w = mount(DirTree, {
      props: { nodes: TREE, selectedId: null, collapsed: new Set([1]) },
    })
    const names = w.findAll('.name').map((n) => n.text())
    expect(names).toEqual(['合同', '安全检查'])
  })
})
