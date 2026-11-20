import { describe, expect, it } from 'vitest'
import { clip, shortPath, splitPath } from './text'

// P2 T5.2：路径中间省略 —— 左边留盘符，右边留「哪一份」
describe('shortPath', () => {
  it('短路径原样返回，不做无意义的省略', () => {
    expect(shortPath('D:\\归档\\合同.docx')).toBe('D:\\归档\\合同.docx')
    expect(shortPath('D:\\a\\b\\c.docx')).toBe('D:\\a\\b\\c.docx')
  })

  it('长路径保留盘符与末两段，中间顶成 …', () => {
    const p = 'C:\\Users\\kingb\\Desktop\\2026\\管理\\安全检查通知.docx'
    expect(shortPath(p)).toBe('C:\\…\\管理\\安全检查通知.docx')
  })

  it('认正斜杠，也保留它自己的风格（含开头的 /）', () => {
    expect(shortPath('/home/kingb/work/2026/第四季度报告归档汇编/q4/报告.pdf')).toBe(
      '/home/…/q4/报告.pdf',
    )
  })

  it('末尾段就是要看的文件名，绝不能被省略掉', () => {
    const p = 'D:\\文档归档\\智能工厂平台\\安全环保\\检查报告\\2026年度消防验收报告（终版）.docx'
    expect(shortPath(p).endsWith('2026年度消防验收报告（终版）.docx')).toBe(true)
  })

  it('空串不炸', () => {
    expect(shortPath('')).toBe('')
  })
})

describe('splitPath / clip', () => {
  it('两种分隔符混用也能拆干净', () => {
    expect(splitPath('C:\\a/b\\c.docx')).toEqual(['C:', 'a', 'b', 'c.docx'])
  })

  it('clip 按字符数裁，省略号占一格', () => {
    expect(clip('一二三四五', 5)).toBe('一二三四五')
    expect(clip('一二三四五六', 5)).toBe('一二三四…')
  })
})
