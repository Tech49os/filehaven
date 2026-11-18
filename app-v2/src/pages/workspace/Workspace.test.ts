import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { BatchView, Candidate } from '../../core/types'

const nextBatch = vi.fn()
const bootState = vi.fn()
const decide = vi.fn()
const closeBatch = vi.fn()
const createDir = vi.fn()
const undoDecide = vi.fn()
const saveNote = vi.fn()

vi.mock('../../core/ipc', () => ({
  nextBatch,
  bootState,
  decide,
  closeBatch,
  createDir,
  undoDecide,
  saveNote,
  listDirs: vi.fn().mockResolvedValue([]),
  previewDirName: vi.fn(),
}))
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
}))

const Workspace = (await import('./Workspace.vue')).default

function cand(dirId: number, relPath: string, uncertain = false): Candidate {
  return { dirId, relPath, score: uncertain ? 0.4 : 0.8, reason: `${relPath} 的匹配理由`, uncertain }
}

function view(partial: Partial<BatchView> = {}): BatchView {
  return {
    batchId: 42,
    seq: 3,
    size: 12,
    items: [],
    remainingInPool: 37,
    dirCount: 2,
    deferredBadge: [],
    ...partial,
  }
}

function item(id: number, name: string, candidates: Candidate[], needsYou = false) {
  return {
    file: {
      id,
      name,
      ext: 'docx',
      kind: 'word' as const,
      size: 20480,
      mtime: Date.now() - 3600_000,
      status: 'pending_confirm' as const,
      relPath: `C:/桌面/${name}`,
      absPath: `C:/桌面/${name}`,
      sourceRootId: 1,
      deferCount: 0,
      note: null,
    },
    candidates,
    needsYou,
  }
}

describe('整理台渲染（T6.9 / T6.10 / §8.4）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    nextBatch.mockReset()
    bootState.mockReset().mockResolvedValue({
      configured: true,
      archiveRoot: 'D:/文档归档',
      sourceRoots: [{ id: 1, path: 'C:/桌面', enabled: true }],
      counts: {
        queued: 0,
        labeled: 0,
        pending_confirm: 12,
        confirmed: 0,
        migrated: 0,
        parked: 0,
        skipped: 0,
        missing: 0,
      },
      model: { reachable: true, model: 'qwen2.5:7b' },
    })
    decide.mockResolvedValue({
      fileId: 1,
      status: 'confirmed',
      confirmed: 1,
      targetRelPath: '安全检查',
      remainingInPool: 37,
    })
  })

  it('顶部显示「第 N 批 · 份数 · 已确认 n/N」与池内剩余', async () => {
    nextBatch.mockResolvedValue(
      view({
        items: [
          item(1, '安全检查通知.docx', [cand(7, '安全检查')]),
          item(2, '采购合同.docx', [cand(8, '合同', true)]),
        ],
      }),
    )
    const w = mount(Workspace)
    await flushPromises()

    const top = w.find('.title').text()
    expect(top).toContain('第 3 批')
    expect(top).toContain('2 份')
    expect(top).toContain('已确认 0')
    expect(w.text()).toContain('池里还有 37 份')
    // 卡片上每份都要有理由，不许出现「无理由」的候选
    expect(w.text()).toContain('安全检查 的匹配理由')
    // 切到第 2 份，不确定的候选要有橙色徽标
    await w.findAll('.qrow')[1].trigger('click')
    expect(w.text()).toContain('不太确定')
  })

  it('needsYou 显示「待你指定」并提示只能新建/暂缓', async () => {
    nextBatch.mockResolvedValue(
      view({ items: [item(1, '哈希名.docx', [], true), item(2, 'b.docx', [cand(8, '合同')])] }),
    )
    const w = mount(Workspace)
    await flushPromises()
    expect(w.text()).toContain('待你指定')
    expect(w.text()).toContain('这份等你定')
    // 主按钮退化成「新建目录」
    expect(w.find('.mid .ft .btn.primary').text()).toContain('新建目录')
  })

  it('0 目录空态：给出两条出路，且主按钮改成新建目录（验收 11）', async () => {
    nextBatch.mockResolvedValue(
      view({ dirCount: 0, items: [item(1, 'a.docx', [], true)] }),
    )
    const w = mount(Workspace)
    await flushPromises()
    expect(w.text()).toContain('还没有任何目录')
    expect(w.text()).toContain('让 AI 建议目录结构')
    expect(w.text()).toContain('手动新建目录')
    expect(w.text()).toContain('先按')
    expect(w.find('.mid .ft .btn.primary').text()).toContain('新建目录')
  })

  it('点确认后卡片变绿勾、计数 +1、焦点跳下一份', async () => {
    nextBatch.mockResolvedValue(
      view({
        items: [
          item(1, '安全检查通知.docx', [cand(7, '安全检查')]),
          item(2, '采购合同.docx', [cand(8, '合同')]),
        ],
      }),
    )
    const w = mount(Workspace)
    await flushPromises()

    await w.find('.mid .ft .btn.primary').trigger('click')
    await flushPromises()

    expect(decide).toHaveBeenCalledTimes(1)
    expect(decide.mock.calls[0][0]).toMatchObject({
      fileId: 1,
      batchId: 42,
      dirId: 7,
      decision: 'accepted',
    })
    expect(w.find('.title').text()).toContain('已确认 1/2')
    expect(w.find('aside.right').text()).toContain('安全检查通知.docx')
    // 焦点跳到第 2 份
    expect(w.find('.name').text()).toBe('采购合同.docx')
  })

  it('点「撤销」按钮：调 undo_decide 并把计数退回去', async () => {
    nextBatch.mockResolvedValue(
      view({
        items: [
          item(1, '安全检查通知.docx', [cand(7, '安全检查')]),
          item(2, '采购合同.docx', [cand(8, '合同')]),
        ],
      }),
    )
    const w = mount(Workspace)
    await flushPromises()

    await w.find('.mid .ft .btn.primary').trigger('click')
    await flushPromises()
    expect(w.find('.title').text()).toContain('已确认 1/2')

    const undoBtn = w.findAll('.top button').find((b) => b.text().includes('撤销'))
    expect(undoBtn).toBeTruthy()
    expect(undoBtn!.attributes('disabled')).toBeUndefined()

    undoDecide.mockResolvedValue(item(1, '安全检查通知.docx', [cand(7, '安全检查')]))
    await undoBtn!.trigger('click')
    await flushPromises()

    expect(undoDecide).toHaveBeenCalledWith(42)
    expect(w.find('.title').text()).toContain('已确认 0/2')
    expect(w.find('.name').text()).toBe('安全检查通知.docx')
  })

  it('池空时给出下一步入口', async () => {
    nextBatch.mockResolvedValue(view({ items: [] }))
    const w = mount(Workspace)
    await flushPromises()
    expect(w.text()).toContain('池子里没有待整理的文件了')
  })
})
