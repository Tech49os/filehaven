import { describe, expect, it } from 'vitest'
import { cleanDirName, cleanPath, md5Hex } from './dirname'
import cases from '../../test/fixtures/dirname-cases.json'

interface Case {
  input: string
  cleaned: string
  valid: boolean
  reason?: string
}

describe('cleanDirName（与 Rust 共跑同一份 JSON）', () => {
  it('20 条用例全部一致', () => {
    const list = cases as Case[]
    expect(list.length).toBe(20)
    for (const c of list) {
      const p = cleanDirName(c.input)
      expect(p.cleaned, `input=${JSON.stringify(c.input)}`).toBe(c.cleaned)
      expect(p.valid, `input=${JSON.stringify(c.input)}`).toBe(c.valid)
    }
  })

  it('md5 与标准实现一致（RFC 1321 测试向量）', () => {
    expect(md5Hex('')).toBe('d41d8cd98f00b204e9800998ecf8427e')
    expect(md5Hex('abc')).toBe('900150983cd24fb0d6963f7d28e17f72')
    expect(md5Hex('中文')).toBe(md5Hex('中文'))
  })

  it('多段路径先拆再清洗', () => {
    expect(cleanPath('安全/检查:报告').cleaned).toBe('安全/检查_报告')
    expect(cleanPath('  / 安全 // 报告 ').cleaned).toBe('安全/报告')
    expect(cleanPath('.../报告').valid).toBe(false)
  })
})
