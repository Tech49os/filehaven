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
const previewFile = vi.fn()
const openPath = vi.fn()
const revealPath = vi.fn()
const renameFile = vi.fn()
const rescoreFiles = vi.fn()

vi.mock('../../core/ipc', () => ({
  nextBatch,
  bootState,
  decide,
  closeBatch,
  createDir,
  undoDecide,
  saveNote,
  previewFile,
  openPath,
  revealPath,
  renameFile,
  rescoreFiles,
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
  /**
   * jsdom 的视口是 1024 宽，`(max-width: 1240px)` 天然为真 ——
   * 不显式指定的话每条用例都跑在「窄窗抽屉」模式下，就测不到右栏并排的样子了。
   */
  function matchMediaAs(matches: boolean) {
    return vi.fn().mockImplementation(() => ({
      matches,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }))
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    vi.stubGlobal('matchMedia', matchMediaAs(false))
    nextBatch.mockReset()
    previewFile.mockReset().mockResolvedValue({
      text: '',
      truncated: false,
      reason: '',
      kind: 'text',
    })
    openPath.mockReset().mockResolvedValue(undefined)
    revealPath.mockReset().mockResolvedValue(undefined)
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

  // 2026-11-20 反馈：「第多少批哪些没必要，就待确认多少份即可」——
  // 顶部只报「还要定几份」，不再提第几批
  it('顶部只显示「待确认 N 份」与池内剩余，不谈批次', async () => {
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
    expect(top).toBe('待确认 2 份')
    expect(top).not.toContain('第 3 批')
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
    expect(w.text()).toContain('自己写一个')
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
    expect(w.find('.title').text()).toBe('待确认 1 份')
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
    expect(w.find('.title').text()).toBe('待确认 1 份')

    const undoBtn = w.findAll('.top button').find((b) => b.text().includes('撤销'))
    expect(undoBtn).toBeTruthy()
    expect(undoBtn!.attributes('disabled')).toBeUndefined()

    undoDecide.mockResolvedValue(item(1, '安全检查通知.docx', [cand(7, '安全检查')]))
    await undoBtn!.trigger('click')
    await flushPromises()

    expect(undoDecide).toHaveBeenCalledWith(42)
    expect(w.find('.title').text()).toBe('待确认 2 份')
    expect(w.find('.name').text()).toBe('安全检查通知.docx')
  })

  it('池空时给出下一步入口', async () => {
    nextBatch.mockResolvedValue(view({ items: [] }))
    const w = mount(Workspace)
    await flushPromises()
    expect(w.text()).toContain('池子里没有待整理的文件了')
  })

  // ------------------------------------------------ 看内容 / 打开（2026-11-20 反馈）

  it('当前这份会自动取一次正文预览，同一份不重复取', async () => {
    previewFile.mockResolvedValue({
      text: '甲方：某某公司，合同金额十二万元',
      truncated: false,
      reason: '',
      kind: 'text',
    })
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

    expect(previewFile).toHaveBeenCalledTimes(1)
    expect(previewFile).toHaveBeenCalledWith(1)
    expect(w.text()).toContain('合同金额十二万元')

    // 回车确认后焦点跳到第 2 份 → 再取第 2 份的
    previewFile.mockResolvedValue({
      text: '',
      truncated: false,
      reason: 'PDF 这一版不抽正文，直接打开看最快',
      kind: 'none',
    })
    await w.find('.mid .ft .btn.primary').trigger('click')
    await flushPromises()
    expect(previewFile).toHaveBeenCalledWith(2)

    // 回到第 1 份时用缓存，不再打后端
    await w.findAll('.qrow')[0].trigger('click')
    await flushPromises()
    expect(previewFile).toHaveBeenCalledTimes(2)
  })

  it('点「打开」调系统默认程序，点「定位」调资源管理器', async () => {
    previewFile.mockResolvedValue({
      text: '',
      truncated: false,
      reason: '这类文件读不出正文',
      kind: 'none',
    })
    nextBatch.mockResolvedValue(view({ items: [item(1, '采购合同.docx', [cand(8, '合同')])] }))
    const w = mount(Workspace)
    await flushPromises()

    await w.findAll('button').find((b) => b.text().includes('打开'))!.trigger('click')
    expect(openPath).toHaveBeenCalledWith('C:/桌面/采购合同.docx')

    await w.findAll('button').find((b) => b.text().includes('定位'))!.trigger('click')
    expect(revealPath).toHaveBeenCalledWith('C:/桌面/采购合同.docx')
  })

  it('窗口窄时右栏收进抽屉，按钮能开合', async () => {
    // 1160 的默认窗口减去侧栏只剩 ~900，三栏并排会把中间卡片挤成一条
    vi.stubGlobal('matchMedia', matchMediaAs(true))
    nextBatch.mockResolvedValue(view({ items: [item(1, 'a.docx', [cand(7, '安全检查')])] }))
    const w = mount(Workspace)
    await flushPromises()

    expect(w.find('.ws').classes()).toContain('narrow')
    expect(w.find('aside.right').exists()).toBe(false)

    const btn = w.findAll('button').find((b) => b.text().includes('已确认'))!
    expect(btn).toBeTruthy()
    await btn.trigger('click')
    expect(w.find('aside.right').exists()).toBe(true)
  })

  // ------------------------------------------------ 复用批次 / 重新推荐 / 改名（2026-11-20 反馈）

  it('切走再回来接着用同一批，不重开批次', async () => {
    nextBatch.mockResolvedValue(
      view({ items: [item(1, 'a.docx', [cand(7, '安全检查')]), item(2, 'b.docx', [cand(8, '合同')])] }),
    )
    const first = mount(Workspace)
    await flushPromises()
    expect(nextBatch).toHaveBeenCalledTimes(1)
    // 确认第一份，制造「用户已经干过活」的状态
    await first.find('.mid .ft .btn.primary').trigger('click')
    await flushPromises()
    first.unmount()

    const again = mount(Workspace)
    await flushPromises()
    expect(nextBatch).toHaveBeenCalledTimes(1) // 关键：没有再开一批
    expect(again.find('.title').text()).toBe('待确认 1 份')
    expect(again.find('aside.right').text()).toContain('a.docx')
  })

  it('点「重新推荐」只重算还没定的那几份', async () => {
    rescoreFiles.mockResolvedValue([
      { file: item(2, 'b.docx', []).file, candidates: [cand(9, '合同/2026年')], needsYou: false },
    ])
    nextBatch.mockResolvedValue(
      view({
        items: [item(1, 'a.docx', [cand(7, '安全检查')]), item(2, 'b.docx', [cand(8, '合同')])],
      }),
    )
    const w = mount(Workspace)
    await flushPromises()
    await w.find('.mid .ft .btn.primary').trigger('click')
    await flushPromises()

    const btn = w.findAll('button').find((b) => b.text() === '重新推荐')!
    await btn.trigger('click')
    await flushPromises()

    expect(rescoreFiles).toHaveBeenCalledWith([2])
    expect(w.text()).toContain('1 份的首选换了')
    expect(w.text()).toContain('合同/2026年')
  })

  it('确认前能给文件改名，源文件名不动、卡片上说清「归档时存为」', async () => {
    renameFile.mockResolvedValue('生产智能化项目组工作汇报-终版.pptx')
    nextBatch.mockResolvedValue(view({ items: [item(1, 'IMG_2043.docx', [cand(7, '安全检查')])] }))
    const w = mount(Workspace)
    await flushPromises()

    await w.find('.ren').trigger('click')
    const input = w.find('.rename-input')
    expect(input.exists()).toBe(true)
    // 输入框里只有主干，扩展名单独固定在右边
    expect((input.element as HTMLInputElement).value).toBe('IMG_2043')
    expect(w.find('.rename-ext').text()).toBe('.docx')

    await input.setValue('生产智能化项目组工作汇报-终版')
    await input.trigger('keydown', { key: 'Enter' })
    await flushPromises()

    expect(renameFile).toHaveBeenCalledWith(1, '生产智能化项目组工作汇报-终版')
    expect(w.text()).toContain('归档时存为')
    expect(w.text()).toContain('生产智能化项目组工作汇报-终版.pptx')
    // 原名还在（磁盘上的名字没动）
    expect(w.find('.name').text()).toBe('IMG_2043.docx')
  })

  it('0 目录提示是一行，不再占掉大半屏', async () => {
    nextBatch.mockResolvedValue(view({ dirCount: 0, items: [item(1, 'a.docx', [], true)] }))
    const w = mount(Workspace)
    await flushPromises()
    expect(w.find('.nodir').exists()).toBe(true)
    expect(w.find('.nodir').text()).toContain('还没有任何目录')
    // 大块 EmptyState 不该再出现
    expect(w.find('.mid').text()).not.toContain('先有目录才能归档')
  })

  it('空批次不算有批次：扫到文件后再进来要重开', async () => {
    nextBatch.mockResolvedValueOnce(view({ items: [] }))
    const empty = mount(Workspace)
    await flushPromises()
    expect(empty.text()).toContain('池子里没有待整理的文件了')
    empty.unmount()

    // 扫描补进来两份
    nextBatch.mockResolvedValue(
      view({ items: [item(1, 'a.docx', [cand(7, '安全检查')])] }),
    )
    const again = mount(Workspace)
    await flushPromises()
    expect(nextBatch).toHaveBeenCalledTimes(2)
    expect(again.find('.name').text()).toBe('a.docx')
  })
})
