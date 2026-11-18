import { describe, it, expect } from 'vitest'
import { isNonDocument } from '../src/core/skip'

describe('扫描时该跳过的文件', () => {
  it('快捷方式 / 可执行程序不进来', () => {
    expect(isNonDocument('FileManager.lnk')).toBe(true)
    expect(isNonDocument('Firefox.exe')).toBe(true)
    expect(isNonDocument('setup.msi')).toBe(true)
  })

  it('系统文件不进来', () => {
    expect(isNonDocument('desktop.ini')).toBe(true)
    expect(isNonDocument('Thumbs.db')).toBe(true)
  })

  it('压缩包不进来（读不出正文，留着只会堆成一坨未归类）', () => {
    expect(isNonDocument('资料.zip')).toBe(true)
    expect(isNonDocument('备份.7z')).toBe(true)
  })

  it('真正的文档都保留', () => {
    for (const n of [
      '技术谱系汇报.docx',
      '成本清单.xlsx',
      '汇报.pptx',
      '合同.pdf',
      '架构图.drawio',
      '备注.md',
      '安环大屏.png',
      '数据.csv',
    ]) {
      expect(isNonDocument(n)).toBe(false)
    }
  })

  it('扩展名大小写不影响判断', () => {
    expect(isNonDocument('A.EXE')).toBe(true)
    expect(isNonDocument('B.Docx')).toBe(false)
  })

  it('无扩展名的文件保留（可能是有内容的文件，不做武断判断）', () => {
    expect(isNonDocument('README')).toBe(false)
  })
})
