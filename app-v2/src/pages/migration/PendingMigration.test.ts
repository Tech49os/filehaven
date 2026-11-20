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
        pendingConfirm: 0,
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

  // 2026-11-20 第三轮反馈：「待迁移区文件多了之后排版错乱」。
  // 直接原因是行里多了一个「改名」标签 —— 它自己占了一个格子，
  // 于是带改名的行和别的行行不对齐，越往下越乱。
  it('带改名的行也不多占一格：每行永远只有四格', async () => {
    const renamed: FileRow = { ...file(4, 'IMG_2043.docx', 512), targetName: '配电箱巡检记录.docx' }
    migrationPreview.mockResolvedValue(
      preview({
        groups: [
          {
            dirId: 7,
            relPath: '安全检查',
            isNew: false,
            files: [file(1, '安全检查通知.docx', 2048), renamed],
            bytes: 2560,
          },
        ],
        nameConflicts: [],
        totalFiles: 2,
      }),
    )
    const w = mount(PendingMigration)
    await flushPromises()

    const rows = w.findAll('.frow')
    expect(rows.length).toBe(2)
    for (const r of rows) {
      // 勾选框 / 名字（含改名标签）/ 体积 / 状态
      expect(r.element.children.length).toBe(4)
    }
    // 标签必须待在名字格里，跟着名字走
    expect(rows[0].find('.fnamewrap .tag').exists()).toBe(false)
    expect(rows[1].find('.fnamewrap .tag').text()).toBe('改名')
    expect(rows[1].find('.fnamewrap .fname').text()).toBe('配电箱巡检记录.docx')
    // 体积与状态各自落在自己的格子里
    const cells = Array.from(rows[1].element.children).map((c) => c.className)
    expect(cells).toEqual(['', 'fnamewrap', 'fsize', 'fst'])
  })

  // 同一份清单里既有带标签的又有不带标签的，四种格子必须都是同一个位置
  it('混排时体积与状态列不错位', async () => {
    const renamed: FileRow = { ...file(4, 'b.docx', 512), targetName: '新名字.docx' }
    migrationPreview.mockResolvedValue(
      preview({
        groups: [
          {
            dirId: 7,
            relPath: '合同',
            isNew: false,
            files: [
              file(1, 'a.docx', 2048),
              renamed,
              { ...file(2, 'c.docx', 256), targetName: '另一个新名.docx' },
            ],
            bytes: 2816,
          },
        ],
        nameConflicts: [],
        totalFiles: 3,
      }),
    )
    const w = mount(PendingMigration)
    await flushPromises()
    // 三行的「四格结构」一致，靠 CSS 的固定列宽就自然对齐
    const shapes = w.findAll('.frow').map((r) =>
      Array.from(r.element.children).map((c) => c.className),
    )
    expect(shapes[0].length).toBe(4)
    expect(shapes[1].length).toBe(4)
    expect(shapes[2].length).toBe(4)
    expect(shapes[1][1]).toBe('fnamewrap')
  })
})
