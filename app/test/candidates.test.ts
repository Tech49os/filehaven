import { describe, it, expect } from 'vitest'
import { collectCandidates, type FileRef } from '../src/core/emergence/candidates'

const F = (name: string, path = ''): FileRef => ({ name, path })

const find = (cs: ReturnType<typeof collectCandidates>, term: string) =>
  cs.find((c) => c.term === term)

describe('候选专名抽取', () => {
  it('抽出跨文件重复出现的专名', () => {
    const files = [
      F('智能感知技术谱系汇报材料-初稿v1.2-智能工厂技术室.pptx'),
      F('软件开发技术谱系-更新-一名改.docx'),
      F('技术谱系-人工智能部分修订.docx'),
    ]
    const t = find(collectCandidates(files), '技术谱系')
    expect(t).toBeDefined()
    expect(t!.df).toBe(3)
  })

  it('把部门名标记成组织名', () => {
    const files = [
      F('智能工厂技术室-人员转型方案.docx'),
      F('智能工厂技术室运维组管理规定202609.docx'),
      F('智能工厂技术室-开发组全员全栈转型.pdf'),
    ]
    const org = find(collectCandidates(files), '智能工厂技术室')
    expect(org).toBeDefined()
    expect(org!.isOrg).toBe(true)
  })

  it('以组织后缀结尾但属于真实事项的短词不被误判为组织名', () => {
    const files = [F('油化厂-现场bug.docx'), F('油化厂项目方案.docx'), F('油化厂验收单.xlsx')]
    const t = find(collectCandidates(files), '油化厂')
    expect(t).toBeDefined()
    expect(t!.isOrg).toBe(false)
  })

  it('过于普遍的高频词被丢弃', () => {
    // 需要足够多的文件才会启用 8% 上限（小样本下任何重复都可能是真事项）
    const files: FileRef[] = []
    for (let i = 0; i < 20; i++) {
      files.push(F(`海油发展平台-材料${i}.docx`))
    }
    for (let i = 0; i < 8; i++) {
      files.push(F(`其他事项-文件${i}.docx`))
    }
    const all = collectCandidates(files, { maxDfRatio: 0.5 })
    expect(find(all, '海油发展平台')).toBeUndefined()
  })

  it('只出现一次的词不进候选', () => {
    const files = [F('孤立的词甲乙丙.docx'), F('另一份材料-技术谱系.docx'), F('技术谱系-补充.docx')]
    const cs = collectCandidates(files)
    expect(find(cs, '孤立的词甲乙丙')).toBeUndefined()
    expect(find(cs, '技术谱系')).toBeDefined()
  })

  it('同族词只保留最完整的一个，碎片被剔除', () => {
    const files = [
      F('科技技术谱系-智能感知.docx'),
      F('科技技术谱系-软件开发.docx'),
      F('科技技术谱系-人工智能.docx'),
    ]
    const cs = collectCandidates(files)
    expect(find(cs, '科技技术谱系')).toBeDefined()
    // 从长段里切出来的碎片不该占候选名额
    expect(find(cs, '科技技')).toBeUndefined()
    expect(find(cs, '技术谱')).toBeUndefined()
  })

  it('短形态本身是完整段时，优先于把它当子串的长形态', () => {
    const files = [
      F('智能感知技术谱系汇报材料-初稿v1.2-智能工厂技术室.pptx'),
      F('软件开发技术谱系-更新-一名改.docx'),
      F('技术谱系-人工智能部分修订.docx'),
    ]
    const cs = collectCandidates(files)
    // 「技术谱系」在这批文件里是完整段，且 df 最高
    expect(find(cs, '技术谱系')).toBeDefined()
  })

  it('真实事项排在组织名前面', () => {
    const files = [
      F('智能感知技术谱系汇报材料-智能工厂技术室.pptx'),
      F('软件开发技术谱系-智能工厂技术室.docx'),
      F('技术谱系-智能工厂技术室.pdf'),
    ]
    const cs = collectCandidates(files)
    const iTopic = cs.findIndex((c) => c.term === '技术谱系')
    const iOrg = cs.findIndex((c) => c.term === '智能工厂技术室')
    expect(iTopic).toBeGreaterThanOrEqual(0)
    expect(iOrg).toBeGreaterThan(iTopic)
  })

  it('结果不超过 topK', () => {
    const files: FileRef[] = []
    for (let i = 0; i < 60; i++) {
      files.push(F(`事项${i % 20}号材料-补充说明${i}.docx`))
    }
    const cs = collectCandidates(files, { topK: 10 })
    expect(cs.length).toBeLessThanOrEqual(10)
  })

  it('给模型看的样本不超过 3 个，且是原始文件名', () => {
    const files = [
      F('技术谱系-甲.docx'),
      F('技术谱系-乙.docx'),
      F('技术谱系-丙.docx'),
      F('技术谱系-丁.docx'),
    ]
    const t = find(collectCandidates(files), '技术谱系')!
    expect(t.samples.length).toBeLessThanOrEqual(3)
    expect(t.samples.every((s) => s.endsWith('.docx'))).toBe(true)
  })
})
