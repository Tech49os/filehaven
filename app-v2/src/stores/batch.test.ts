import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { BatchItem, BatchView, Candidate } from '../core/types'

const nextBatch = vi.fn()
const decide = vi.fn()
const undoDecide = vi.fn()
const closeBatch = vi.fn()
const createDir = vi.fn()
const saveNote = vi.fn()
vi.mock('../core/ipc', () => ({ nextBatch, decide, undoDecide, closeBatch, createDir, saveNote }))

const { useBatchStore } = await import('./batch')

function cand(dirId: number, relPath: string, score = 0.8): Candidate {
  return { dirId, relPath, score, reason: `${relPath} 的理由`, uncertain: score < 0.55 }
}

function item(id: number, candidates: Candidate[]): BatchItem {
  return {
    file: {
      id,
      name: `文件${id}.docx`,
      ext: 'docx',
      kind: 'word',
      size: 1024,
      mtime: 1_700_000_000_000,
      status: 'pending_confirm',
      relPath: `C:/桌面/文件${id}.docx`,
      absPath: `C:/桌面/文件${id}.docx`,
      sourceRootId: 1,
      deferCount: 0,
      note: null,
    },
    candidates,
    needsYou: candidates.length === 0,
  }
}

function view(items: BatchItem[]): BatchView {
  return {
    batchId: 42,
    seq: 3,
    size: 12,
    items,
    remainingInPool: 30,
    dirCount: 2,
    deferredBadge: [],
  }
}

describe('batch store（T6.14 判定与焦点）', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    nextBatch.mockReset()
    decide.mockReset()
    undoDecide.mockReset()
    closeBatch.mockReset()
    nextBatch.mockResolvedValue(
      view([item(1, [cand(7, '安全检查')]), item(2, [cand(8, '合同')]), item(3, [])]),
    )
    decide.mockImplementation(async (req: { fileId: number; decision: string }) => ({
      fileId: req.fileId,
      status:
        req.decision === 'deferred' ? 'queued' : req.decision === 'parked' ? 'parked' : 'confirmed',
      confirmed: req.decision === 'deferred' || req.decision === 'parked' ? 0 : 1,
      targetRelPath: null,
      remainingInPool: 30,
    }))
  })

  it('loading 后停在第 0 份', async () => {
    const s = useBatchStore()
    await s.load()
    expect(s.total).toBe(3)
    expect(s.index).toBe(0)
    expect(s.current?.file.id).toBe(1)
    expect(s.dirCount).toBe(2)
  })

  it('accept 用主推目录，卡片变 confirmed 且焦点跳到下一份', async () => {
    const s = useBatchStore()
    await s.load()
    await s.accept()
    expect(decide).toHaveBeenCalledTimes(1)
    const req = decide.mock.calls[0][0]
    expect(req.decision).toBe('accepted')
    expect(req.dirId).toBe(7)
    expect(req.origin).toBe('recommended')
    expect(req.batchId).toBe(42)
    expect(s.items[0].file.status).toBe('confirmed')
    expect(s.confirmedCount).toBe(1)
    expect(s.index).toBe(1)
  })

  it('pick(2) 有备选时用第 2 个，origin 记 user_picked', async () => {
    const s = useBatchStore()
    nextBatch.mockResolvedValue(
      view([item(1, [cand(7, '安全检查'), cand(8, '合同')]), item(2, [])]),
    )
    await s.load()
    await s.pick(2)
    const req = decide.mock.calls[0][0]
    expect(req.dirId).toBe(8)
    expect(req.origin).toBe('user_picked')
    expect(req.decision).toBe('changed')
    expect(s.items[0].file.status).toBe('confirmed')
  })

  it('pick(3) 但没有第 3 个候选时不动作（不许误确认）', async () => {
    const s = useBatchStore()
    await s.load()
    await s.pick(3)
    expect(decide).not.toHaveBeenCalled()
    expect(s.index).toBe(0)
  })

  it('暂缓：状态回 queued、不计入已确认、也往下走', async () => {
    const s = useBatchStore()
    await s.load()
    await s.defer()
    const req = decide.mock.calls[0][0]
    expect(req.decision).toBe('deferred')
    expect(req.dirId).toBeNull()
    expect(s.items[0].file.status).toBe('queued')
    expect(s.confirmedCount).toBe(0)
    expect(s.index).toBe(1)
  })

  it('先放着：parked，候选保留不重复问', async () => {
    const s = useBatchStore()
    await s.load()
    await s.park()
    expect(decide.mock.calls[0][0].decision).toBe('parked')
    expect(s.items[0].file.status).toBe('parked')
    expect(s.index).toBe(1)
  })

  it('撤销：Ctrl+Z 回填该文件并把焦点移回去', async () => {
    const s = useBatchStore()
    await s.load()
    await s.accept()
    expect(s.index).toBe(1)
    undoDecide.mockResolvedValue({ ...s.items[0], file: { ...s.items[0].file, status: 'pending_confirm' } })
    await s.undo()
    expect(undoDecide).toHaveBeenCalledWith(42)
    expect(s.items[0].file.status).toBe('pending_confirm')
    expect(s.index).toBe(0)
    expect(s.confirmedCount).toBe(0)
  })

  it('空栈时 undo 不发请求', async () => {
    const s = useBatchStore()
    await s.load()
    await s.undo()
    expect(undoDecide).not.toHaveBeenCalled()
  })

  it('move 在 0..n-1 之间夹紧，不环绕', async () => {
    const s = useBatchStore()
    await s.load()
    s.move(-1)
    expect(s.index).toBe(0)
    s.move(1)
    s.move(1)
    s.move(1)
    expect(s.index).toBe(2)
  })

  it('全部确认后 allDone=true，finish 关闭批次', async () => {
    const s = useBatchStore()
    await s.load()
    await s.accept()
    await s.accept()
    await s.park()
    expect(s.allDone).toBe(true)
    closeBatch.mockResolvedValue({ closed: 0, remainingInPool: 30 })
    await s.finish()
    expect(closeBatch).toHaveBeenCalledWith(42)
    expect(s.finished?.remainingInPool).toBe(30)
  })

  it('needsYou 时 accept 不发请求（必须新建目录或选备选）', async () => {
    const s = useBatchStore()
    nextBatch.mockResolvedValue(view([item(9, [])]))
    await s.load()
    expect(s.current?.needsYou).toBe(true)
    await s.accept()
    expect(decide).not.toHaveBeenCalled()
  })

  it('方向键移动会清掉输入态（避免键盘被输入框吃掉）', async () => {
    const s = useBatchStore()
    await s.load()
    s.openInput()
    expect(s.inputOpen).toBe(true)
    s.move(1)
    expect(s.inputOpen).toBe(false)
  })
})
