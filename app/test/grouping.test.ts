import { describe, it, expect } from 'vitest'
import { groupBySurface } from '../src/core/emergence/grouping'
import type { Candidate } from '../src/core/emergence/candidates'

const C = (term: string, df = 5, isOrg = false): Candidate => ({
  term, df, isOrg, weak: false, samples: [`${term}.docx`], score: df,
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

  it('组织名不当事项：留成兜底类，不当正经类', () => {
    const g = groupBySurface([C('生产智能化项目组', 30, true), C('技术谱系', 18)])
    const org = g.find((x) => x.name === '生产智能化项目组')!
    const real = g.find((x) => x.name === '技术谱系')!
    expect(real.weak).toBe(false)
    expect(org.weak).toBe(true)
    expect(org.weakReason).toBe('who')
  })

  it('分组按权重排序', () => {
    const g = groupBySurface([C('小事项', 3), C('大事项', 20)])
    expect(g[0].name).toBe('大事项')
  })
})

describe('组织/人名类是兜底类，不是事项', () => {
  const who = new Set(['海油发展', '海油发展智能工厂'])
  const cands = [C('海油发展', 27), C('海油发展智能工厂', 7), C('技术谱系', 17)]

  it('组名是人名/组织名的组被标为兜底（不删，留给用户决定）', () => {
    const g = groupBySurface(cands, { who })
    const org = g.find((x) => x.name === '海油发展智能工厂')!
    expect(org).toBeDefined()
    expect(org.weak).toBe(true)
    expect(org.weakReason).toBe('who')
  })

  it('真事项不受影响，仍是正经类', () => {
    const g = groupBySurface(cands, { who })
    const real = g.find((x) => x.name === '技术谱系')!
    expect(real.weak).toBe(false)
  })

  it('不传 who 时行为不变（降级路径）', () => {
    const g = groupBySurface([C('海油发展', 27), C('技术谱系', 17)])
    expect(g.map((x) => x.name).sort()).toEqual(['技术谱系', '海油发展'].sort())
    expect(g.every((x) => !x.weak)).toBe(true)
  })
})
