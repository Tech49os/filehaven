import { describe, expect, it, vi, beforeEach } from 'vitest'

const invoke = vi.fn().mockResolvedValue(null)
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const ipc = await import('./ipc')

describe('ipc 参数键必须是 camelCase（对齐 Rust serde rename_all）', () => {
  beforeEach(() => invoke.mockClear())

  it('decide 传 { req } 且字段是 fileId 而不是 file_id', () => {
    void ipc.decide({
      fileId: 1,
      batchId: 9,
      dirId: 3,
      decision: 'accepted',
      origin: 'recommended',
      confidence: 0.8,
      reason: '文件名与目录高度重合',
    })
    const [cmd, args] = invoke.mock.calls[0]
    expect(cmd).toBe('decide')
    expect(Object.keys(args)).toEqual(['req'])
    expect(args.req.fileId).toBe(1)
    expect(args.req.batchId).toBe(9)
    expect(args.req.dirId).toBe(3)
    expect(args.req).not.toHaveProperty('file_id')
    expect(args.req).not.toHaveProperty('batch_id')
  })

  it('undo_decide / close_batch 传 batchId', () => {
    void ipc.undoDecide(9)
    expect(invoke.mock.calls[0]).toEqual(['undo_decide', { batchId: 9 }])
    invoke.mockClear()
    void ipc.closeBatch(9)
    expect(invoke.mock.calls[0]).toEqual(['close_batch', { batchId: 9 }])
  })

  it('preview_dir_name / create_dir 传 input，accept_dirs 传 names', () => {
    void ipc.previewDirName('安全/检查')
    expect(invoke.mock.calls[0]).toEqual(['preview_dir_name', { input: '安全/检查' }])
    invoke.mockClear()
    void ipc.acceptDirs(['安全', '合同'])
    expect(invoke.mock.calls[0]).toEqual(['accept_dirs', { names: ['安全', '合同'] }])
  })

  it('save_note 传 fileId + note', () => {
    void ipc.saveNote(5, '等财务确认')
    expect(invoke.mock.calls[0]).toEqual(['save_note', { fileId: 5, note: '等财务确认' }])
  })
})
