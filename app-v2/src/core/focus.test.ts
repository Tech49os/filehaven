import { describe, expect, it } from 'vitest'
import { nextUnconfirmed, popUndo, pushUndo } from './focus'
import type { BatchItem } from './types'

function item(id: number, status: BatchItem['file']['status']): BatchItem {
  return {
    file: {
      id,
      name: `f${id}.docx`,
      ext: 'docx',
      kind: 'word',
      size: 100,
      mtime: 0,
      status,
      relPath: `C:/t/f${id}.docx`,
      absPath: `C:/t/f${id}.docx`,
      sourceRootId: 1,
      deferCount: 0,
      note: null,
    },
    candidates: [],
    needsYou: true,
  }
}

describe('nextUnconfirmed', () => {
  it('从 0 跳到 3（0~2 已确认）', () => {
    const items = [
      item(1, 'confirmed'),
      item(2, 'confirmed'),
      item(3, 'confirmed'),
      item(4, 'pending_confirm'),
      item(5, 'pending_confirm'),
    ]
    expect(nextUnconfirmed(items, 0)).toBe(3)
  })

  it('已确认项跳过：从中间往后找', () => {
    const items = [
      item(1, 'pending_confirm'),
      item(2, 'confirmed'),
      item(3, 'pending_confirm'),
    ]
    expect(nextUnconfirmed(items, 1)).toBe(2)
  })

  it('全部确认返回 -1', () => {
    const items = [item(1, 'confirmed'), item(2, 'confirmed')]
    expect(nextUnconfirmed(items, 0)).toBe(-1)
    expect(nextUnconfirmed([], 0)).toBe(-1)
  })

  it('暂缓/先放着不算已确认，仍会被选中', () => {
    const items = [
      item(1, 'queued'),
      item(2, 'parked'),
      item(3, 'confirmed'),
    ]
    expect(nextUnconfirmed(items, 0)).toBe(0)
    expect(nextUnconfirmed(items, 1)).toBe(1)
  })
})

describe('撤销栈', () => {
  it('push 后 pop 是后进先出', () => {
    let s: number[] = []
    s = pushUndo(s, 11)
    s = pushUndo(s, 22)
    s = pushUndo(s, 33)
    const a = popUndo(s)
    expect(a.fileId).toBe(33)
    const b = popUndo(a.rest)
    expect(b.fileId).toBe(22)
    expect(b.rest).toEqual([11])
  })

  it('空栈 pop 返回 null，栈不变', () => {
    const r = popUndo([])
    expect(r.fileId).toBeNull()
    expect(r.rest).toEqual([])
  })

  it('同一文件重复 push 只留一条（连按两次确认不该撤销两次）', () => {
    let s: number[] = []
    s = pushUndo(s, 7)
    s = pushUndo(s, 7)
    expect(s).toEqual([7])
  })
})
