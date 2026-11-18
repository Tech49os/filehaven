import { describe, it, expect } from 'vitest'
import { countTier, sortForDisplay } from '../src/core/emergence/view'

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
