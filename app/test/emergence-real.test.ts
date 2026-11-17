import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'

const here = dirname(fileURLToPath(import.meta.url))
const data = JSON.parse(readFileSync(resolve(here, 'real-classify.json'), 'utf-8'))
const files = data.details.map((d: any) => ({ name: d.name, path: d.rel }))
const cands = collectCandidates(files, { topK: 200 })
const terms = cands.map((c) => c.term)
const byTerm = new Map(cands.map((c) => [c.term, c]))

/**
 * 真实 450 份文件回归 —— 对应规格里验收标准 1 / 2 / 3 / 8
 *
 * 这些断言不是「好看」，而是防止以后调参数时把涌现能力改坏：
 * 一旦某个改动让真事项消失或让碎片回来，这里会立刻红。
 */
describe('真实 450 份文件回归', () => {
  it('能涌现出真实事项名（不靠任何行业词表）', () => {
    const must = ['技术谱系', '人工智能推进会', '钻采院']
    for (const m of must) {
      expect(terms, `缺少真实事项「${m}」`).toContain(m)
    }
    // 同一件事的不同写法，命中任意一种即可（长形态更具体，也算对）
    expect(terms.some((t) => t.includes('梯度培育'))).toBe(true)
    expect(terms.some((t) => t.includes('逐浪数字蓝海'))).toBe(true)
    expect(terms.some((t) => t.includes('AI大赛') || t.includes('AI编程'))).toBe(true)
  })

  it('部门名被标记成组织名', () => {
    for (const org of ['生产智能化项目组', '智能工厂技术室', '信科公司']) {
      const c = byTerm.get(org)
      if (!c) continue
      expect(c.isOrg, `「${org}」应被标记为组织名`).toBe(true)
    }
  })

  it('滑动窗口碎片已被剔除', () => {
    const fragments = ['中海油信息科技有', '海油信息科技有限', '油信息科技有限公', '有限公司科技技术']
    for (const f of fragments) {
      expect(terms, `碎片「${f}」不该出现在候选里`).not.toContain(f)
    }
  })

  it('候选数量在可控范围（够模型读，又不至于淹没它）', () => {
    expect(cands.length).toBeGreaterThan(50)
    expect(cands.length).toBeLessThanOrEqual(200)
  })

  it('每条候选自带不超过 3 个代表文件名', () => {
    for (const c of cands) {
      expect(c.samples.length).toBeLessThanOrEqual(3)
      expect(c.samples.length).toBeGreaterThan(0)
    }
  })
})

describe('通用性守卫：源码里不许出现行业词', () => {
  it('emergence 目录下不存在任何行业词常量', () => {
    const dir = resolve(here, '../src/core/emergence')
    const banned = ['钻采', '油化厂', '智能工厂', '海油', '信科', '谱系', '化工']
    for (const f of readdirSync(dir).filter((n) => n.endsWith('.ts'))) {
      const src = readFileSync(resolve(dir, f), 'utf-8')
      for (const w of banned) {
        // 注释里可以提到（作为反例说明），只看非注释行
        const codeLines = src
          .split('\n')
          .filter((l) => !l.trim().startsWith('*') && !l.trim().startsWith('//') && !l.trim().startsWith('/*'))
          .join('\n')
        expect(codeLines, `${f} 的代码里出现了行业词「${w}」，这会让工具失去通用性`).not.toContain(w)
      }
    }
  })
})
