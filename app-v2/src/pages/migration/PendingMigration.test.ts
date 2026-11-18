import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { FileRow, MigrationPreview, MigrationResult } from '../../core/types'

const migrationPreview = vi.fn()
const latestMigration = vi.fn()
const applyMigration = vi.fn()
const undoMigration = vi.fn()
const requeueConfirmed = vi.fn()
const bootState = vi.fn()

vi.mock('../../core/ipc', () => ({
  migrationPreview,
  latestMigration,
  applyMigration,
  undoMigration,
  requeueConfirmed,
  bootState,
}))
vi.mock('../../core/events', () => ({
  onMigrateProgress: async () => () => {},
  onUndoProgress: async () => () => {},
}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))

const PendingMigration = (await import('./PendingMigration.vue')).default

function file(id: number, name: string, size: number): FileRow {
  return {
    id,
    name,
    ext: name.split('.').pop() ?? 'docx',
    kind: 'word',
    size,
    mtime: Date.now() - 86400_000,
    status: 'confirmed',
    relPath: `C:/桌面/${name}`,
    absPath: `C:/桌面/${name}`,
    sourceRootId: 1,
    deferCount: 0,
    note: null,
  }
}

function preview(partial: Partial<MigrationPreview> = {}): MigrationPreview {
  return {
    groups: [
      {
        dirId: 7,
        relPath: '安全检查',
        isNew: true,
        files: [file(1, '安全检查通知.docx', 2048), file(2, '检查表.xlsx', 1024)],
        bytes: 3072,
      },
      {
        dirId: 8,
        relPath: '采购合同',
        isNew: false,
        files: [file(3, '采购合同.docx', 4096)],
        bytes: 4096,
      },
    ],
    nameConflicts: [{ fileId: 3, targetName: '采购合同_2.docx' }],
    changedSinceLabel: [],
    totalFiles: 3,
    totalBytes: 7168,
    freeGb: 120.5,
    enoughSpace: true,
    ...partial,
  }
}

const APPLIED: MigrationResult = {
  migrationId: 9,
  copied: 3,
  verified: 3,
  quarantined: 3,
  failed: [],
}

describe('待迁移页（T7.6）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    migrationPreview.mockReset()
    latestMigration.mockReset().mockResolvedValue(null)
    applyMigration.mockReset().mockResolvedValue(APPLIED)
    undoMigration.mockReset().mockResolvedValue({ restored: 3, skipped: 0, failed: [] })
    requeueConfirmed.mockReset().mockResolvedValue(undefined)
    bootState.mockReset().mockResolvedValue({
      configured: true,
      archiveRoot: 'D:/文档归档',
      sourceRoots: [{ id: 1, path: 'C:/桌面', enabled: true }],
      counts: {
        queued: 0,
        labeled: 0,
        pending_confirm: 0,
        confirmed: 3,
        migrated: 0,
        parked: 0,
        skipped: 0,
        missing: 0,
      },
      model: { reachable: false, model: null },
    })
  })

  it('顶部汇总份数与字节，分组显示并标出新建目录与同名冲突', async () => {
    migrationPreview.mockResolvedValue(preview())
    const w = mount(PendingMigration)
    await flushPromises()

    const sum = w.find('.sum').text()
    expect(sum).toContain('3 份')
    expect(sum).toContain('7.0KB')
    expect(w.text()).toContain('安全检查')
    expect(w.text()).toContain('采购合同')
    expect(w.text()).toContain('新建目录')
    // 同名冲突要说出来，不许静默改名
    expect(w.text()).toContain('采购合同_2.docx')
  })

  it('取消勾选后份数与字节实时变，应用时作为排除列表传给后端', async () => {
    migrationPreview.mockResolvedValue(preview())
    const w = mount(PendingMigration)
    await flushPromises()
    expect(w.find('.count').text()).toContain('3 份')

    // 第 2 份（检查表.xlsx，1024 字节）取消勾选
    const boxes = w.findAll('.frow input[type="checkbox"]')
    expect(boxes.length).toBe(3)
    await boxes[1].setValue(false)
    await flushPromises()

    expect(w.find('.count').text()).toContain('2 份')
    expect(w.find('.count').text()).toContain('6.0KB')

    await w.find('.foot .btn.primary').trigger('click')
    await flushPromises()
    expect(applyMigration).toHaveBeenCalledWith([2])
    expect(w.text()).toContain('已迁移 3 份')
  })

  it('有「标注后已改」时红色拦截、按钮禁用，单份可重新确认', async () => {
    migrationPreview.mockResolvedValue(
      preview({
        changedSinceLabel: [{ fileId: 5, name: '改了.docx', why: 'mtime' }],
        totalFiles: 2,
      }),
    )
    const w = mount(PendingMigration)
    await flushPromises()

    expect(w.find('.warn').text()).toContain('改了.docx')
    expect(w.find('.warn').text()).toContain('修改时间')
    const btn = w.find('.foot .btn.primary')
    expect(btn.attributes('disabled')).toBeDefined()

    const requeueBtn = w.find('.warn button')
    expect(requeueBtn.text()).toContain('重新确认')
    await requeueBtn.trigger('click')
    await flushPromises()
    expect(requeueConfirmed).toHaveBeenCalledWith(5)
  })

  it('空间不够时也要拦住', async () => {
    migrationPreview.mockResolvedValue(preview({ enoughSpace: false, freeGb: 0.1 }))
    const w = mount(PendingMigration)
    await flushPromises()
    expect(w.find('.foot').text()).toContain('空间')
    expect(w.find('.foot .btn.primary').attributes('disabled')).toBeDefined()
  })

  it('迁移完可以撤销，撤销参数是这次迁移的 id', async () => {
    migrationPreview.mockResolvedValue(preview())
    const w = mount(PendingMigration)
    await flushPromises()
    await w.find('.foot .btn.primary').trigger('click')
    await flushPromises()

    expect(w.text()).toContain('已迁移 3 份')
    migrationPreview.mockResolvedValue(preview({ groups: [], totalFiles: 0, totalBytes: 0, nameConflicts: [] }))
    const undoBtn = w.findAll('.result button').find((b) => b.text().includes('撤销'))
    expect(undoBtn).toBeTruthy()
    await undoBtn!.trigger('click')
    await flushPromises()
    expect(undoMigration).toHaveBeenCalledWith(9)
    expect(w.text()).toContain('已还原 3 份')
  })

  it('没有待迁移的文件时给出去处，不白屏', async () => {
    migrationPreview.mockResolvedValue(
      preview({ groups: [], nameConflicts: [], totalFiles: 0, totalBytes: 0 }),
    )
    const w = mount(PendingMigration)
    await flushPromises()
    expect(w.text()).toContain('没有待迁移的文件')
  })
})
