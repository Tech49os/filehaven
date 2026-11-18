/**
 * 分域（归档第一层目录）的纯逻辑测试。
 *
 * 这一层解决的问题：45 个事项平铺在归档根目录下，打开一看就是一堵墙。
 * 先分 2-6 个大域（工作在看哪些方向的文件），再在域下放事项，
 * 目录树才像人整理过的东西。
 */
import { describe, expect, it } from 'vitest'
import {
  buildDomainPrompt,
  buildTree,
  domainsFromSharedToken,
  isUsable,
  parseDomainResult,
} from '../src/core/emergence/domain'

const T = (name: string, weight: number) => ({ name, weight })

const REAL = [
  T('人工智能推进会', 25),
  T('技术谱系', 17),
  T('数智化项目建设', 12),
  T('服务器领用交接单', 11),
  T('AI编程', 11),
  T('软件开发', 10),
  T('智能制造', 9),
  T('智能工厂技术室', 8),
  T('智能工厂项目', 7),
  T('智能工厂产品模块', 6),
  T('智能工厂开展汇报', 4),
  T('培训项目项目方案', 7),
  T('梯度培育培训课程设置', 5),
  T('作业H5培训游戏', 3),
  T('服务器配置', 4),
  T('服务器资源申请单', 3),
  T('境内差旅费', 4),
  T('业务联络函', 3),
]

describe('提示词', () => {
  it('把每个事项都列进去，并给出份数', () => {
    const p = buildDomainPrompt(REAL)
    for (const t of REAL) {
      expect(p).toContain(t.name)
    }
    expect(p).toContain('25')
  })

  it('明确边界：先分域再归类，不许编造事项', () => {
    const p = buildDomainPrompt(REAL)
    expect(p).toContain('2–6')
    expect(p).toMatch(/不许编造|不要编造/)
    expect(p).toContain('JSON')
  })

  it('只给名字和份数，不塞文件名——多字段就多耗时', () => {
    const p = buildDomainPrompt(REAL)
    expect(p.length).toBeLessThan(4000)
  })

  it('空列表也能生成（不崩），但会要求不返回空', () => {
    expect(() => buildDomainPrompt([])).not.toThrow()
  })
})

describe('解析模型输出', () => {
  const names = REAL.map((t) => t.name)

  it('标准输出：declared domains + map 数组', () => {
    const raw = JSON.stringify({
      domains: ['技术研发', '行政事务'],
      map: [
        { topic: '人工智能推进会', domain: '技术研发' },
        { topic: '境内差旅费', domain: '行政事务' },
      ],
    })
    const r = parseDomainResult(raw, names)
    expect(r.domains).toEqual(['技术研发', '行政事务'])
    expect(r.assign.get('人工智能推进会')).toBe('技术研发')
    expect(r.assign.get('境内差旅费')).toBe('行政事务')
  })

  it('容忍对象形式的 map', () => {
    const raw = JSON.stringify({
      domains: ['技术研发'],
      map: { 技术谱系: '技术研发' },
    })
    expect(parseDomainResult(raw, names).assign.get('技术谱系')).toBe('技术研发')
  })

  it('容忍模型在名字里插空格（AI 编程 ≠ AI编程）', () => {
    const raw = JSON.stringify({
      domains: ['技术研发'],
      map: [{ topic: 'AI 编程', domain: '技术研发' }],
    })
    expect(parseDomainResult(raw, names).assign.get('AI编程')).toBe('技术研发')
  })

  it('丢掉编造的事项名和没声明过的大类——不许模型自创目录', () => {
    const raw = JSON.stringify({
      domains: ['技术研发'],
      map: [
        { topic: '这个事项根本不存在', domain: '技术研发' },
        { topic: '技术谱系', domain: '我瞎编的域' },
      ],
    })
    const r = parseDomainResult(raw, names)
    expect(r.assign.size).toBe(0)
  })

  it('JSON 前后有解释文字也能抠出来', () => {
    const raw = `好的，分析如下：
{"domains":["技术研发"],"map":[{"topic":"技术谱系","domain":"技术研发"}]}
以上。`
    expect(parseDomainResult(raw, names).assign.get('技术谱系')).toBe('技术研发')
  })

  it('彻底不是 JSON 就返回空，不抛错', () => {
    const r = parseDomainResult('我不知道', names)
    expect(r.domains).toEqual([])
    expect(r.assign.size).toBe(0)
  })

  it('域数量去重、去空', () => {
    const raw = JSON.stringify({ domains: ['技术研发', '技术研发', '  ', '行政'], map: [] })
    expect(parseDomainResult(raw, names).domains).toEqual(['技术研发', '行政'])
  })
})

describe('可用性判定', () => {
  const names = REAL.map((t) => t.name)
  const mk = (domains: string[], assign: [string, string][]) => ({
    domains,
    assign: new Map(assign),
  })

  it('只有一个大类不算分域（等于没分）', () => {
    const all = names.map((n) => [n, '技术研发'] as [string, string])
    expect(isUsable(mk(['技术研发'], all), names)).toBe(false)
  })

  it('覆盖不到一半的事项，说明模型没答完，退回规则', () => {
    expect(
      isUsable(mk(['甲', '乙'], [['技术谱系', '甲'], ['AI编程', '乙']]), names)
    ).toBe(false)
  })

  it('一个域吃掉九成事项 = 形同虚设', () => {
    const assign: [string, string][] = names.map((n, i) => [n, i === 0 ? '行政' : '技术'])
    expect(isUsable(mk(['技术', '行政'], assign), names)).toBe(false)
  })

  it('正常分布可用', () => {
    const assign: [string, string][] = names.map((n, i) => [n, i % 2 ? '行政' : '技术'])
    expect(isUsable(mk(['技术', '行政'], assign), names)).toBe(true)
  })
})

describe('规则兜底：按名字里共同的开头分组', () => {
  it('4 个「智能工厂X」归到一个大类，大类名就是公共开头', () => {
    const r = domainsFromSharedToken(REAL)
    expect(r.domains).toContain('智能工厂')
    expect(r.assign.get('智能工厂项目')).toBe('智能工厂')
    expect(r.assign.get('智能工厂技术室')).toBe('智能工厂')
  })

  it('凑不出 3 个同类就不硬分——宁可不分，也不造一个只有一个孩子的目录', () => {
    const r = domainsFromSharedToken([
      T('服务器配置', 4),
      T('服务器', 3),
      T('境内差旅费', 4),
    ])
    expect(r.domains).toEqual([])
  })

  it('事项名自己就是公共开头时，不把自己归到自己底下', () => {
    const r = domainsFromSharedToken([
      T('智能工厂', 5),
      T('智能工厂项目', 3),
      T('智能工厂技术室', 3),
    ])
    expect(r.assign.get('智能工厂')).toBeUndefined()
    expect(r.assign.get('智能工厂项目')).toBe('智能工厂')
  })

  it('空的输入返回空结果', () => {
    const r = domainsFromSharedToken([])
    expect(r.domains).toEqual([])
    expect(r.assign.size).toBe(0)
  })
})

describe('建树', () => {
  const topics = [
    { id: 'a', name: '智能工厂项目', domain: '智能工厂', fileIds: ['1', '2'] },
    { id: 'b', name: '智能工厂技术室', domain: '智能工厂', fileIds: ['3'] },
    { id: 'c', name: '境内差旅费', fileIds: ['4'] },
  ]

  it('有域的事项进域，没域的留在一级', () => {
    const tr = buildTree(topics)
    expect(tr.domains.map((d) => d.name)).toEqual(['智能工厂'])
    expect(tr.domains[0].topics.map((t) => t.id)).toEqual(['a', 'b'])
    expect(tr.domains[0].fileCount).toBe(3)
    expect(tr.loose.map((t) => t.id)).toEqual(['c'])
  })

  it('域按文件数降序，域内事项也按文件数降序', () => {
    const tr = buildTree([
      ...topics,
      { id: 'd', name: '人工智能推进会', domain: 'AI', fileIds: ['5', '6', '7', '8'] },
      { id: 'e', name: 'AI编程', domain: 'AI', fileIds: ['9'] },
    ])
    expect(tr.domains.map((d) => d.name)).toEqual(['AI', '智能工厂'])
    expect(tr.domains[1].topics.map((t) => t.id)).toEqual(['a', 'b'])
  })

  it('只有一个事项的域沉成一级事项——单孩子目录只是多一层点击', () => {
    const tr = buildTree([
      { id: 'a', name: '孤零零', domain: '只有它', fileIds: ['1'] },
      { id: 'c', name: '境内差旅费', fileIds: ['4'] },
    ])
    expect(tr.domains).toEqual([])
    expect(tr.loose.map((t) => t.id).sort()).toEqual(['a', 'c'])
  })

  it('域名字重复合并，不产生两个同名目录', () => {
    const tr = buildTree([
      { id: 'a', name: 'x', domain: '技术', fileIds: ['1'] },
      { id: 'b', name: 'y', domain: '技术', fileIds: ['2'] },
      { id: 'c', name: 'z', domain: '技术', fileIds: ['3'] },
    ])
    expect(tr.domains.length).toBe(1)
    expect(tr.domains[0].fileCount).toBe(3)
  })
})
