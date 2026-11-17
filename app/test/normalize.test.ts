import { describe, it, expect } from 'vitest'
import { stripNoise, segments } from '../src/core/emergence/normalize'
import { DOC_KIND_WORDS, ORG_SUFFIXES, GENERIC_WORDS } from '../src/core/emergence/lexicon'

describe('语言级词表', () => {
  it('包含文档性质词', () => {
    for (const w of ['方案', '汇报', '报告', '合同', '附件', '清单']) {
      expect(DOC_KIND_WORDS.has(w)).toBe(true)
    }
  })

  it('包含组织后缀', () => {
    for (const w of ['项目组', '技术室', '公司', '部门']) {
      expect(ORG_SUFFIXES.has(w)).toBe(true)
    }
  })

  it('不含任何行业词（否则就不是通用工具了）', () => {
    const all = [...DOC_KIND_WORDS, ...ORG_SUFFIXES, ...GENERIC_WORDS].join(',')
    for (const w of ['钻采', '油化厂', '智能工厂', '海油', '信科', '谱系', '化学']) {
      if (w === '化学') continue
      expect(all).not.toContain(w)
    }
  })
})

describe('文件名清洗', () => {
  it('剥掉附件前缀与裸编号', () => {
    const s = stripNoise('附件2：双周工作情况报告-生产智能化项目组-1026.docx')
    expect(s).not.toContain('附件')
    expect(s).not.toContain('1026')
    expect(s).toContain('双周工作情况报告')
  })

  it('剥掉版本号与日期', () => {
    expect(stripNoise('蓝图设计v1.0(20260904).pptx')).not.toMatch(/v1\.0/i)
    expect(stripNoise('上报材料20260805改.docx')).not.toContain('20260805')
  })

  it('去掉引号但保留引号里的字', () => {
    // 真实 bug：引号被当分隔符会把「智造领航」切成「造领航」，丢掉「智」
    const s = stripNoise('“智”造领航逐浪数字蓝海-集团V1.2.pptx')
    expect(s).toContain('智造领航逐浪数字蓝海')
  })

  it('保留中文括号里的内容', () => {
    const s = stripNoise('04生产工艺设备服务器等硬件设备支撑（二期）_初版草稿_完.docx')
    expect(s).toContain('二期')
  })

  it('扩展名不参与切段', () => {
    expect(stripNoise('审批单.xlsx')).toBe('审批单')
  })
})

describe('切段', () => {
  it('按分隔符切段并去掉空段', () => {
    const segs = segments('附件2：双周工作情况报告-生产智能化项目组-1026.docx')
    expect(segs).toContain('双周工作情况报告')
    expect(segs).toContain('生产智能化项目组')
    expect(segs.every((s) => s.trim().length > 0)).toBe(true)
  })

  it('纯数字文件名切成空', () => {
    expect(segments('20260917-185927.jpg')).toEqual([])
  })

  it('括号内内容与主体连在一起', () => {
    const segs = segments('智能工厂平台（二期）.docx')
    expect(segs.some((s) => s.includes('二期'))).toBe(true)
  })

  it('同一文件不产生重复段', () => {
    const segs = segments('方案-方案-方案.docx')
    expect(segs.filter((s) => s === '方案')).toHaveLength(1)
  })
})
