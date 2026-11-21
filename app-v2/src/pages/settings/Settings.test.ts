import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

const bootState = vi.fn()
const listDirs = vi.fn()
const diskFreeGb = vi.fn()
const pickDirectory = vi.fn()
const addSourceRoot = vi.fn()
const removeSourceRoot = vi.fn()
const scanAll = vi.fn()
const push = vi.fn()
const deepTagStatus = vi.fn()
const setDeepTag = vi.fn()
const setWatchEnabled = vi.fn()
const listTags = vi.fn()
const deleteTag = vi.fn()
const renameTag = vi.fn()

vi.mock('../../core/ipc', () => ({
  bootState,
  listDirs,
  diskFreeGb,
  pickDirectory,
  addSourceRoot,
  removeSourceRoot,
  scanAll,
  deepTagStatus,
  setDeepTag,
  setWatchEnabled,
  listTags,
  deleteTag,
  renameTag,
  createDir: vi.fn(),
  previewDirName: vi.fn(),
}))
vi.mock('../../core/events', () => ({
  onScanProgress: vi.fn().mockResolvedValue(() => {}),
  onDeepTagProgress: vi.fn().mockResolvedValue(() => {}),
  onFilesFound: vi.fn().mockResolvedValue(() => {}),
}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push, replace: push }) }))

const Settings = (await import('./Settings.vue')).default

function boot(roots: { id: number; path: string; enabled: boolean }[]) {
  return {
    configured: roots.length > 0,
    archiveRoot: 'D:/文档归档',
    sourceRoots: roots,
    counts: {
      queued: 0,
      labeled: 0,
      pendingConfirm: 0,
      confirmed: 0,
      migrated: 0,
      parked: 0,
      skipped: 0,
      missing: 0,
    },
    model: { reachable: true, model: 'qwen2.5:7b' },
  }
}

const btn = (w: ReturnType<typeof mount>, label: string) =>
  w.findAll('button').find((b) => b.text().includes(label))

describe('设置页 · 源区（回归：移除后加不回来）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset()
    listDirs.mockReset().mockResolvedValue([])
    diskFreeGb.mockReset().mockResolvedValue(120.5)
    pickDirectory.mockReset()
    addSourceRoot.mockReset()
    removeSourceRoot.mockReset()
  })

  it('源区卡片有「添加源区」入口，选了目录就落库并刷新', async () => {
    bootState.mockResolvedValueOnce(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    const w = mount(Settings)
    await flushPromises()

    const add = btn(w, '添加源区')
    expect(add, '源区卡片必须有添加入口，否则移除后就锁死了').toBeTruthy()

    pickDirectory.mockResolvedValueOnce('D:/工作')
    bootState.mockResolvedValueOnce(
      boot([
        { id: 1, path: 'C:/桌面', enabled: true },
        { id: 2, path: 'D:/工作', enabled: true },
      ]),
    )
    await add!.trigger('click')
    await flushPromises()

    expect(pickDirectory).toHaveBeenCalled()
    expect(addSourceRoot).toHaveBeenCalledWith('D:/工作')
    expect(w.text()).toContain('D:/工作')
  })

  it('移除最后一个源区后，空态里仍然能添加（原本的死锁路径）', async () => {
    bootState.mockResolvedValueOnce(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    const w = mount(Settings)
    await flushPromises()

    bootState.mockResolvedValueOnce(boot([]))
    const rm = btn(w, '移除')
    await rm!.trigger('click')
    await flushPromises()
    expect(removeSourceRoot).toHaveBeenCalledWith(1)
    expect(w.text()).toContain('还没有源区')

    // 空态也要有入口：卡片头的 + 空态里的链接，至少两处能点
    expect(w.findAll('button').filter((b) => b.text().includes('添加源区')).length).toBeGreaterThan(0)

    pickDirectory.mockResolvedValueOnce('C:/下载')
    bootState.mockResolvedValueOnce(boot([{ id: 3, path: 'C:/下载', enabled: true }]))
    await btn(w, '添加源区')!.trigger('click')
    await flushPromises()
    expect(addSourceRoot).toHaveBeenCalledWith('C:/下载')
    expect(w.text()).toContain('C:/下载')
  })

  it('用户在选目录对话框里取消，不发 IPC', async () => {
    bootState.mockResolvedValueOnce(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    const w = mount(Settings)
    await flushPromises()

    pickDirectory.mockResolvedValueOnce(null)
    await btn(w, '添加源区')!.trigger('click')
    await flushPromises()
    expect(addSourceRoot).not.toHaveBeenCalled()
  })
})

describe('设置页 · 目录结构', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset().mockResolvedValue(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    listDirs.mockReset().mockResolvedValue([])
    diskFreeGb.mockReset().mockResolvedValue(120.5)
    push.mockReset()
  })

  // 目录建议以前只在引导里跑一次，配置完成后 App.vue 就把 /onboarding 重定向走，
  // 于是「换个分法」永远没有入口 —— 想再让 AI 给一版只能删库。
  it('配置完成后仍能回到 AI 目录建议', async () => {
    const w = mount(Settings)
    await flushPromises()
    const entry = btn(w, 'AI 建议目录')
    expect(entry).toBeTruthy()
    await entry!.trigger('click')
    expect(push).toHaveBeenCalledWith('/onboarding/dirs')
  })
})

// ---------------------------------------------------------------- 深度解读队列（P2 T2.8）

describe('设置页 · 深度解读队列', () => {
  const stats = (over: Record<string, unknown> = {}) => ({
    done: 12,
    failed: 0,
    remaining: 30,
    current: '',
    running: false,
    paused: false,
    enabled: true,
    note: null,
    ...over,
  })

  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset().mockResolvedValue(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    listDirs.mockReset().mockResolvedValue([])
    diskFreeGb.mockReset().mockResolvedValue(120.5)
    deepTagStatus.mockReset().mockResolvedValue(stats())
    setDeepTag.mockReset()
  })

  it('关掉开关会把 enabled=false 写回去（重启后仍然是用户的意愿）', async () => {
    const w = mount(Settings)
    await flushPromises()

    setDeepTag.mockResolvedValueOnce(stats({ enabled: false }))
    await btn(w, '关闭')!.trigger('click')
    await flushPromises()
    expect(setDeepTag).toHaveBeenCalledWith({ enabled: false })
    expect(w.text()).toContain('已关闭')
  })

  it('失败数 >0 时明确说出来，不静默', async () => {
    deepTagStatus.mockResolvedValueOnce(stats({ failed: 3 }))
    const w = mount(Settings)
    await flushPromises()
    expect(w.text()).toContain('失败 3')
    expect(w.text()).toContain('没读出来')
  })

  it('暂停后按钮文案变「继续」', async () => {
    const w = mount(Settings)
    await flushPromises()
    const pause = btn(w, '暂停')!
    expect(pause.text()).toBe('暂停')

    setDeepTag.mockResolvedValueOnce(stats({ paused: true }))
    await pause.trigger('click')
    await flushPromises()
    expect(setDeepTag).toHaveBeenCalledWith({ paused: true })
    expect(btn(w, '继续')).toBeTruthy()
  })
})

// ---------------------------------------------------------------- 新文件监听（P2 T3.7）

describe('设置页 · 新文件监听开关', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    listDirs.mockReset().mockResolvedValue([])
    diskFreeGb.mockReset().mockResolvedValue(120.5)
    deepTagStatus.mockReset().mockResolvedValue({
      done: 0,
      failed: 0,
      remaining: 0,
      current: '',
      running: false,
      paused: false,
      enabled: true,
      note: null,
    })
    setWatchEnabled.mockReset()
  })

  it('开着的源区显示「正在盯着源区」，可以关掉', async () => {
    bootState.mockReset().mockResolvedValue({
      ...boot([{ id: 1, path: 'C:/桌面', enabled: true }]),
      watchEnabled: true,
    })
    setWatchEnabled.mockResolvedValue(undefined)
    const w = mount(Settings)
    await flushPromises()

    expect(w.text()).toContain('正在盯着源区')
    await btn(w, '暂停监听')!.trigger('click')
    await flushPromises()
    expect(setWatchEnabled).toHaveBeenCalledWith(false)
  })

  it('关着的时候说明「还在启动时扫一次」，别让人以为完全不扫了', async () => {
    bootState.mockReset().mockResolvedValue({
      ...boot([{ id: 1, path: 'C:/桌面', enabled: true }]),
      watchEnabled: false,
    })
    const w = mount(Settings)
    await flushPromises()

    expect(w.text()).toContain('已关闭，只在启动时扫一次')
    expect(w.text()).toContain('启动扫描')
    expect(btn(w, '开启监听')).toBeTruthy()
  })
})

// ------------------------------------------------ 标签词表（P3 T1.5）

describe('设置页 · 标签词表', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset().mockResolvedValue(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    listDirs.mockReset().mockResolvedValue([])
    diskFreeGb.mockReset().mockResolvedValue(120.5)
    deepTagStatus.mockReset().mockResolvedValue({
      done: 0, failed: 0, remaining: 0, current: '', running: false,
      paused: false, enabled: false, note: null,
    })
    listTags.mockReset()
    deleteTag.mockReset()
    renameTag.mockReset()
  })

  it('渲染 user/topic 两类标签 + 使用份数', async () => {
    listTags.mockResolvedValue([
      { id: 11, name: '安全检查', kind: 'topic', fileCount: 34 },
      { id: 12, name: '重点跟进', kind: 'user', fileCount: 2 },
    ])
    const w = mount(Settings)
    await flushPromises()

    const rows = w.findAll('.tagrow')
    expect(rows.length).toBe(2)
    expect(w.text()).toContain('安全检查')
    expect(w.text()).toContain('34 份')
    expect(w.text()).toContain('自己加的')
    expect(w.text()).toContain('AI 给的')
  })

  it('删掉一个标签后该行消失（先调命令再刷列表）', async () => {
    listTags.mockResolvedValue([
      { id: 11, name: '安全检查', kind: 'topic', fileCount: 34 },
      { id: 12, name: '重点跟进', kind: 'user', fileCount: 2 },
    ])
    deleteTag.mockResolvedValue(34)
    const w = mount(Settings)
    await flushPromises()

    const row = w.findAll('.tagrow').find((r) => r.text().includes('安全检查'))!
    await row.findAll('button').find((b) => b.text() === '删除')!.trigger('click')
    await flushPromises()

    expect(deleteTag).toHaveBeenCalledWith(11)
    const names = w.findAll('.tagrow').map((r) => r.text())
    // 删掉的行要消失
    expect(names.some((t) => t.includes('安全检查'))).toBe(false)
    expect(w.findAll('.tagrow').length).toBe(1)
  })
})
