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

vi.mock('../../core/ipc', () => ({
  bootState,
  listDirs,
  diskFreeGb,
  pickDirectory,
  addSourceRoot,
  removeSourceRoot,
  scanAll,
  createDir: vi.fn(),
  previewDirName: vi.fn(),
}))
vi.mock('../../core/events', () => ({ onScanProgress: vi.fn().mockResolvedValue(() => {}) }))
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
      pending_confirm: 0,
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
