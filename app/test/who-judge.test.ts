import { describe, it, expect } from 'vitest'
import { buildWhoPrompt, parseWhoResult, expandWho, augmentWithOrgRule } from '../src/core/emergence/whoJudge'
import type { Candidate } from '../src/core/emergence/candidates'
import { isOrgName } from '../src/core/emergence/lexicon'

const C = (term: string, df = 3): Candidate => ({
  term,
  df,
  isOrg: false,
  weak: false,
  samples: [`${term}-v1.docx`],
  score: 1,
})

describe('组织/人名判定：造 prompt', () => {
  it('每个候选都进 prompt，带份数和代表文件名', () => {
    const p = buildWhoPrompt([C('海油发展', 27), C('技术谱系', 17)])
    expect(p).toContain('海油发展 · 27份')
    expect(p).toContain('技术谱系 · 17份')
  })

  it('选项是封闭的三个值，问的是「是不是谁」而不是「归哪一类」', () => {
    const p = buildWhoPrompt([C('集团')])
    expect(p).toContain('person')
    expect(p).toContain('org')
    expect(p).toContain('no')
    expect(p).toContain('人名或组织名')
  })
})

describe('组织/人名判定：解析模型输出', () => {
  const cands = [C('AI编程'), C('作业H5培训游戏'), C('海油发展'), C('技术谱系'), C('集团')]

  it('抹掉模型在英文数字两侧插的空格后再比对', () => {
    const raw = JSON.stringify({
      items: [
        { term: 'AI 编程', kind: 'no' },
        { term: '作业 H5 培训游戏', kind: 'no' },
      ],
    })
    const m = parseWhoResult(raw, cands)
    expect(m.get('AI编程')).toBe('no')
    expect(m.get('作业H5培训游戏')).toBe('no')
  })

  it('编造的词丢弃，非法的 kind 忽略', () => {
    const raw = JSON.stringify({
      items: [
        { term: '海油发展', kind: 'org' },
        { term: '候选里没有的词', kind: 'org' },
        { term: '技术谱系', kind: 'matter' },
      ],
    })
    const m = parseWhoResult(raw, cands)
    expect(m.get('海油发展')).toBe('org')
    expect(m.has('候选里没有的词')).toBe(false)
    expect(m.has('技术谱系')).toBe(false)
  })

  it('没返回 JSON 就当没判过（上层会降级到规则）', () => {
    expect(parseWhoResult('抱歉，我无法完成', cands).size).toBe(0)
  })

  it('kind 大小写和前后空格都容错', () => {
    const raw = JSON.stringify({ items: [{ term: '集团', kind: ' ORG ' }] })
    expect(parseWhoResult(raw, cands).get('集团')).toBe('org')
  })
})

describe('组织/人名判定：前缀扩散', () => {
  const cands = [C('海油发展'), C('海油发展数字化'), C('海油发展智能工厂'), C('技术谱系', 17)]

  it('以组织名开头的复合词也算组织名', () => {
    const base = new Map([['海油发展', 'org' as const], ['技术谱系', 'no' as const]])
    const who = expandWho(base, cands)
    expect(who.has('海油发展')).toBe(true)
    expect(who.has('海油发展数字化')).toBe(true)
    expect(who.has('海油发展智能工厂')).toBe(true)
  })

  it('不是组织名开头的词不受影响', () => {
    const base = new Map([['技术谱系', 'no' as const]])
    const who = expandWho(base, cands)
    expect(who.has('技术谱系')).toBe(false)
    expect(who.size).toBe(0)
  })

  it('过短的前缀不扩散，避免「集团X」把无关词一起杀掉', () => {
    const c2 = [C('集团'), C('集团化运作')]
    const who = expandWho(new Map([['集团', 'org' as const]]), c2)
    // 「集团」本身算，但它只有 2 个字，不做前缀扩散
    expect(who.has('集团')).toBe(true)
    expect(who.has('集团化运作')).toBe(false)
  })
})

describe('组织/人名判定：并上后缀词典的老规则', () => {
  it('模型漏掉的「XX项目组」由规则补上', () => {
    const cands = [C('生产智能化项目组'), C('技术谱系', 17)]
    const merged = augmentWithOrgRule(new Map([['技术谱系', 'no' as const]]), cands)
    expect(merged.get('生产智能化项目组')).toBe('org')
    expect(merged.get('技术谱系')).toBe('no')
  })

  it('模型已经给出判断的词不被规则覆盖', () => {
    const cands = [C('技术部')]
    const merged = augmentWithOrgRule(new Map([['技术部', 'no' as const]]), cands)
    expect(merged.get('技术部')).toBe('no')
  })

  it('并集之后再做前缀扩散，能补到「海油发展数字化」这类复合词', () => {
    const cands = [C('海油发展'), C('海油发展数字化'), C('信科公司'), C('信科公司服务器')]
    const base = augmentWithOrgRule(new Map([['海油发展', 'org' as const]]), cands)
    const who = expandWho(base, cands)
    expect(who.has('海油发展数字化')).toBe(true)
    expect(who.has('信科公司服务器')).toBe(true)
  })
})

describe('光秃秃的组织泛称也算组织名', () => {
  it('「集团」单独出现时是组织名', () => {
    expect(isOrgName('集团')).toBe(true)
    expect(isOrgName('公司')).toBe(true)
  })
  it('带具体专名的组织名判断不受影响', () => {
    expect(isOrgName('技术谱系')).toBe(false)
    expect(isOrgName('人工智能推进会')).toBe(false)
  })
})
