import { describe, it, expect } from 'vitest'
import { buildMergePrompt, parseMergeResult, fallbackMerge } from '../src/core/emergence/merge'
import type { Candidate } from '../src/core/emergence/candidates'

const C = (term: string, df = 5, isOrg = false): Candidate => ({
  term,
  df,
  isOrg,
  samples: [`${term}-样例.docx`],
  score: df,
})

const cands = [
  C('技术谱系', 18),
  C('人工智能推进会', 14),
  C('生产智能化项目组', 30, true),
  C('演讲稿', 7),
  C('逐浪数字蓝海', 12),
  C('智造领航', 12),
]

describe('归并 prompt', () => {
  it('包含全部候选词与代表文件名', () => {
    const p = buildMergePrompt(cands)
    for (const c of cands) expect(p).toContain(c.term)
    expect(p).toContain('技术谱系-样例.docx')
  })

  it('要求逐条分组而不是归纳大类', () => {
    const p = buildMergePrompt(cands)
    expect(p).toContain('不要标 null')
    expect(p).toContain('单独成组')
    expect(p.toUpperCase()).toContain('JSON')
  })
})

describe('归并结果解析', () => {
  it('同一 topic 的词聚合为一个事项', () => {
    const raw = JSON.stringify({
      items: [
        { term: '技术谱系', topic: '技术谱系' },
        { term: '逐浪数字蓝海', topic: '技术谱系' },
        { term: '人工智能推进会', topic: '人工智能推进会' },
      ],
    })
    const r = parseMergeResult(raw, cands)
    expect(r.topics).toHaveLength(2)
    const tx = r.topics.find((t) => t.name === '技术谱系')!
    expect(tx.aliases).toEqual(['技术谱系', '逐浪数字蓝海'])
  })

  it('topic 为 null 的词判为噪音', () => {
    const raw = JSON.stringify({
      items: [
        { term: '技术谱系', topic: '技术谱系' },
        { term: '演讲稿', topic: null },
        { term: '生产智能化项目组', topic: null },
      ],
    })
    const r = parseMergeResult(raw, cands)
    expect(r.topics).toHaveLength(1)
    expect(r.noise).toEqual(['演讲稿', '生产智能化项目组'])
  })

  it('剔除候选池里没有的幻觉 term', () => {
    const raw = JSON.stringify({
      items: [
        { term: '技术谱系', topic: '技术谱系' },
        { term: '彻头彻尾的新词', topic: '技术谱系' },
      ],
    })
    const r = parseMergeResult(raw, cands)
    expect(r.topics[0].aliases).toEqual(['技术谱系'])
    expect(r.rejected).toContain('彻头彻尾的新词')
  })

  it('模型输出不是 JSON 时抛错，交给上层降级', () => {
    expect(() => parseMergeResult('我觉得这些文件都是技术方案', cands)).toThrow()
  })

  it('一条有效事项都没有时抛错', () => {
    expect(() => parseMergeResult('{"items":[]}', cands)).toThrow()
    expect(() =>
      parseMergeResult('{"items":[{"term":"技术谱系","topic":null}]}', cands)
    ).toThrow()
  })
})

describe('降级：模型不可用时', () => {
  it('用非组织候选直接当类名', () => {
    const r = fallbackMerge(cands, 3)
    expect(r.map((t) => t.name)).toEqual(['技术谱系', '人工智能推进会', '逐浪数字蓝海'])
    expect(r.every((t) => t.aliases.length === 1)).toBe(true)
  })

  it('不把组织名当类名', () => {
    const r = fallbackMerge(cands, 10)
    expect(r.map((t) => t.name)).not.toContain('生产智能化项目组')
  })
})
