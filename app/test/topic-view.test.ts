import { describe, it, expect } from 'vitest'
import { countTier, sortForDisplay, groupUngrouped } from '../src/core/emergence/view'

/** 造一个只有份数的分类 */
const T = (name: string, n: number) => ({
  name,
  fileIds: Array.from({ length: n }, (_, i) => `${name}-${i}`),
})

describe('份数档位', () => {
  it('10 份及以上是大类', () => {
    expect(countTier(10)).toBe('major')
    expect(countTier(18)).toBe('major')
  })

  it('4 到 9 份是常规类', () => {
    expect(countTier(4)).toBe('normal')
    expect(countTier(9)).toBe('normal')
  })

  it('3 份及以下是零散类', () => {
    expect(countTier(3)).toBe('minor')
    expect(countTier(1)).toBe('minor')
  })

  it('空类也算零散（删完文件后残留的）', () => {
    expect(countTier(0)).toBe('minor')
  })
})

describe('展示排序', () => {
  it('按份数降序，一眼看出谁是大类', () => {
    const sorted = sortForDisplay([T('小', 2), T('大', 18), T('中', 7)])
    expect(sorted.map((x) => x.name)).toEqual(['大', '中', '小'])
  })

  it('份数相同的保持原顺序，不来回跳', () => {
    const sorted = sortForDisplay([T('甲', 5), T('乙', 5), T('丙', 5)])
    expect(sorted.map((x) => x.name)).toEqual(['甲', '乙', '丙'])
  })

  it('不修改传入的数组', () => {
    const input = [T('小', 2), T('大', 18)]
    sortForDisplay(input)
    expect(input.map((x) => x.name)).toEqual(['小', '大'])
  })

  it('空分类沉到最底部', () => {
    const sorted = sortForDisplay([T('空的', 0), T('小', 2)])
    expect(sorted.map((x) => x.name)).toEqual(['小', '空的'])
  })
})

describe('未归类文件按目录成组（看清还剩什么）', () => {
  it('同一目录的文件归到一起，按份数降序', () => {
    const paths: Record<string, string> = {
      a: String.raw`D:\文档\下载\20260908\a.docx`,
      b: String.raw`D:\文档\下载\20260908\b.docx`,
      c: String.raw`D:\文档\桌面\c.docx`,
    }
    const g = groupUngrouped(['a', 'b', 'c'], (id) => paths[id])
    expect(g[0].label).toBe(String.raw`下载\20260908`)
    expect(g[0].fileIds).toEqual(['a', 'b'])
    expect(g[1].label).toBe(String.raw`文档\桌面`)
  })

  it('只显示末两级目录名，太长的路径没法塞进一行', () => {
    const g = groupUngrouped(['a'], () => String.raw`C:\Users\x\Desktop\归档\下载\20260908\a.docx`)
    expect(g[0].label).toBe(String.raw`归档\下载\20260908`.split('\\').slice(-2).join('\\'))
  })

  it('拿不到路径的文件归到「未知目录」，不丢', () => {
    const g = groupUngrouped(['a'], () => undefined)
    expect(g).toHaveLength(1)
    expect(g[0].label).toBe('未知目录')
    expect(g[0].fileIds).toEqual(['a'])
  })

  it('空列表返回空数组', () => {
    expect(groupUngrouped([], () => undefined)).toEqual([])
  })

  it('分组结果覆盖所有传入的文件，一个不少', () => {
    const ids = ['a', 'b', 'c', 'd']
    const g = groupUngrouped(ids, (id) => `D://x//${id === 'a' ? 'p' : 'q'}\\${id}.docx`)
    expect(g.flatMap((x) => x.fileIds).sort()).toEqual(ids.sort())
  })
})
