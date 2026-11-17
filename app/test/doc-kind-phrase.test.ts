import { describe, it, expect } from 'vitest'
import { isDocKindPhrase } from '../src/core/emergence/lexicon'
import { collectCandidates, type FileRef } from '../src/core/emergence/candidates'

const F = (name: string, path = ''): FileRef => ({ name, path })

describe('文档性质短语气判定', () => {
  it('识别「修饰词 + 文种」的短语气', () => {
    expect(isDocKindPhrase('演讲稿')).toBe(true)
    expect(isDocKindPhrase('进展汇报')).toBe(true)
    expect(isDocKindPhrase('操作手册')).toBe(true)
    expect(isDocKindPhrase('资源申请单')).toBe(true)
    expect(isDocKindPhrase('ppt修改')).toBe(true)
    expect(isDocKindPhrase('人员名单')).toBe(true)
  })

  it('专名 + 文种不算：前缀够长说明是具体那件事', () => {
    expect(isDocKindPhrase('服务器领用交接单')).toBe(false)
    expect(isDocKindPhrase('智能工厂开展汇报')).toBe(false)
    expect(isDocKindPhrase('培训项目项目方案')).toBe(false)
  })

  it('真事项一律不动 —— 宁可漏杀不可误杀', () => {
    expect(isDocKindPhrase('技术谱系')).toBe(false)
    expect(isDocKindPhrase('人工智能推进会')).toBe(false)
    expect(isDocKindPhrase('数字化建设')).toBe(false)
    expect(isDocKindPhrase('境内差旅费')).toBe(false)
    expect(isDocKindPhrase('安环大屏')).toBe(false)
    expect(isDocKindPhrase('技术沉淀')).toBe(false)
    expect(isDocKindPhrase('AI大赛')).toBe(false)
  })
})

describe('候选抽取阶段把文种语气标成 weak（不删，降级）', () => {
  it('「进展汇报」被标 weak', () => {
    const files = [
      F('进展汇报.pptx'),
      F('进展汇报-第三周.pptx'),
      F('进展汇报终版.pptx'),
    ]
    const cs = collectCandidates(files)
    expect(cs.find((c) => c.term === '进展汇报')?.weak).toBe(true)
  })

  it('但「某某项目进展汇报」这种带专名的仍然留下', () => {
    const files = [
      F('智能工厂开展汇报-信科公司.pptx'),
      F('智能工厂开展汇报-0512.pptx'),
      F('智能工厂开展汇报.pptx'),
    ]
    const terms = collectCandidates(files).map((c) => c.term)
    expect(terms).toContain('智能工厂开展汇报')
  })
})
