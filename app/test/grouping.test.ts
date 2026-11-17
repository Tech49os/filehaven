import { describe, it, expect } from 'vitest'
import { groupBySurface } from '../src/core/emergence/grouping'
import type { Candidate } from '../src/core/emergence/candidates'

const C = (term: string, df = 5, isOrg = false): Candidate => ({
  term, df, isOrg, samples: [`${term}.docx`], score: df,
})

describe('保守分组', () => {
  it('把同一件事的简写和全称合到一组', () => {
    const g = groupBySurface([C('智造领航逐浪数字蓝海', 7), C('智造领航', 12), C('逐浪数字蓝海', 12)])
    expect(g).toHaveLength(1)
    expect(g[0].aliases.sort()).toEqual(['智造领航', '智造领航逐浪数字蓝海', '逐浪数字蓝海'].sort())
  })

  it('组名取最具体的写法（最长）', () => {
    const g = groupBySurface([C('梯度培育培训', 12), C('梯度培育培训课程设置', 4)])
    expect(g).toHaveLength(1)
    expect(g[0].name).toBe('梯度培育培训课程设置')
  })

  it('不相关的事各自成组', () => {
    const g = groupBySurface([C('技术谱系', 18), C('梯度培育培训', 12), C('中期工作会', 7)])
    expect(g).toHaveLength(3)
  })

  it('长度差过大的不做合并（避免泛词吞掉具体事项）', () => {
    const g = groupBySurface([C('服务器', 15), C('服务器领用交接单模板说明表', 4)])
    expect(g).toHaveLength(2)
  })

  it('组织名不参与分组', () => {
    const g = groupBySurface([C('生产智能化项目组', 30, true), C('技术谱系', 18)])
    expect(g).toHaveLength(1)
    expect(g[0].name).toBe('技术谱系')
  })

  it('分组按权重排序', () => {
    const g = groupBySurface([C('小事项', 3), C('大事项', 20)])
    expect(g[0].name).toBe('大事项')
  })
})
