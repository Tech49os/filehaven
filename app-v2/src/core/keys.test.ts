import { describe, expect, it } from 'vitest'
import { mapKey, type KeyCmd } from './keys'

const k = (key: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean }> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  ...mods,
})

const ctx = { inputOpen: false }
const typed = { inputOpen: true }

describe('mapKey（详设 §8.3 全 9 键）', () => {
  it('Enter → accept', () => {
    expect(mapKey(k('Enter'), ctx)).toEqual<KeyCmd>({ t: 'accept' })
  })

  it('1 → pick 1', () => {
    expect(mapKey(k('1'), ctx)).toEqual<KeyCmd>({ t: 'pick', n: 1 })
  })

  it('2 → pick 2', () => {
    expect(mapKey(k('2'), ctx)).toEqual<KeyCmd>({ t: 'pick', n: 2 })
  })

  it('3 → pick 3', () => {
    expect(mapKey(k('3'), ctx)).toEqual<KeyCmd>({ t: 'pick', n: 3 })
  })

  it('n / N → newDir（大写也认）', () => {
    expect(mapKey(k('n'), ctx)).toEqual<KeyCmd>({ t: 'newDir' })
    expect(mapKey(k('N'), ctx)).toEqual<KeyCmd>({ t: 'newDir' })
  })

  it('s → defer', () => {
    expect(mapKey(k('s'), ctx)).toEqual<KeyCmd>({ t: 'defer' })
  })

  it('p → park', () => {
    expect(mapKey(k('p'), ctx)).toEqual<KeyCmd>({ t: 'park' })
  })

  it('ArrowDown → move +1，ArrowUp → move -1', () => {
    expect(mapKey(k('ArrowDown'), ctx)).toEqual<KeyCmd>({ t: 'move', d: 1 })
    expect(mapKey(k('ArrowUp'), ctx)).toEqual<KeyCmd>({ t: 'move', d: -1 })
  })

  it('Escape → escape', () => {
    expect(mapKey(k('Escape'), ctx)).toEqual<KeyCmd>({ t: 'escape' })
  })

  it('Ctrl+Z / Cmd+Z → undo（单独的 z 不是撤销）', () => {
    expect(mapKey(k('z', { ctrlKey: true }), ctx)).toEqual<KeyCmd>({ t: 'undo' })
    expect(mapKey(k('z', { metaKey: true }), ctx)).toEqual<KeyCmd>({ t: 'undo' })
    expect(mapKey(k('z'), ctx)).toBeNull()
  })

  it('无关按键返回 null', () => {
    expect(mapKey(k('a'), ctx)).toBeNull()
    expect(mapKey(k('F5'), ctx)).toBeNull()
  })
})

describe('输入框打开时只拦 Esc / Enter（最容易踩的坑）', () => {
  it('inputOpen 时按 s 返回 null（否则新建框里打不出字母）', () => {
    expect(mapKey(k('s'), typed)).toBeNull()
    expect(mapKey(k('p'), typed)).toBeNull()
    expect(mapKey(k('n'), typed)).toBeNull()
    expect(mapKey(k('1'), typed)).toBeNull()
    expect(mapKey(k('ArrowDown'), typed)).toBeNull()
    // Ctrl+Z 在输入框里属于「撤销文本」，不能抢
    expect(mapKey(k('z', { ctrlKey: true }), typed)).toBeNull()
  })

  it('inputOpen 时 Esc → escape，Enter → accept', () => {
    expect(mapKey(k('Escape'), typed)).toEqual<KeyCmd>({ t: 'escape' })
    expect(mapKey(k('Enter'), typed)).toEqual<KeyCmd>({ t: 'accept' })
  })
})
