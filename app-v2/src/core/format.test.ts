import { describe, expect, it } from 'vitest'
import { formatSize, formatTime, kindLabel, relativeTime } from './format'

describe('formatSize', () => {
  it('按 1024 进制分段', () => {
    expect(formatSize(0)).toBe('0B')
    expect(formatSize(999)).toBe('999B')
    expect(formatSize(1024)).toBe('1.0KB')
    expect(formatSize(500 * 1024)).toBe('500KB')
    expect(formatSize(1024 * 1024)).toBe('1.0MB')
    expect(formatSize(6.6 * 1024 ** 3)).toBe('6.6GB')
  })

  it('非法值不崩', () => {
    expect(formatSize(-1)).toBe('—')
    expect(formatSize(Number.NaN)).toBe('—')
  })
})

describe('formatTime / relativeTime', () => {
  const now = new Date('2026-11-18T21:30:00').getTime()

  it('同年只写月日与时分', () => {
    const ms = new Date('2026-03-05T09:08:00').getTime()
    expect(formatTime(ms, now)).toBe('03-05 09:08')
  })

  it('跨年带上年份', () => {
    const ms = new Date('2025-12-31T23:59:00').getTime()
    expect(formatTime(ms, now)).toBe('2025-12-31')
  })

  it('相对时间分档', () => {
    expect(relativeTime(now - 3600_000, now)).toBe('今天')
    expect(relativeTime(new Date('2026-11-17T10:00:00').getTime(), now)).toBe('昨天')
    expect(relativeTime(new Date('2026-11-15T10:00:00').getTime(), now)).toBe('3 天前')
    expect(relativeTime(new Date('2026-10-01T10:00:00').getTime(), now)).toBe('10-01 10:00')
  })
})

describe('kindLabel', () => {
  it('六类文件类型与兜底', () => {
    expect(kindLabel('word')).toBe('文档')
    expect(kindLabel('excel')).toBe('表格')
    expect(kindLabel('pdf')).toBe('PDF')
    expect(kindLabel('unknown')).toBe('其他')
  })
})
