import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

const pickDirectory = vi.fn()
const addSourceRoot = vi.fn()
const removeSourceRoot = vi.fn()
const setArchiveRoot = vi.fn()
const diskFreeGb = vi.fn().mockResolvedValue(120.5)
const scanAll = vi.fn()
const replace = vi.fn()

vi.mock('../../core/ipc', () => ({
  pickDirectory,
  addSourceRoot,
  removeSourceRoot,
  setArchiveRoot,
  diskFreeGb,
  scanAll,
  bootState: vi.fn(),
  listSourceRoots: vi.fn(),
}))
vi.mock('../../core/events', () => ({
  onScanProgress: async () => () => {},
}))
vi.mock('vue-router', () => ({
  useRouter: () => ({ replace, push: replace }),
}))

const Onboarding = (await import('./Onboarding.vue')).default
const { useAppStore } = await import('../../stores/app')

const EMPTY = {
  queued: 0,
  labeled: 0,
  pendingConfirm: 0,
  confirmed: 0,
  migrated: 0,
  parked: 0,
  skipped: 0,
  missing: 0,
}

function setBoot(sources: { id: number; path: string; enabled: boolean }[], configured = false) {
  const app = useAppStore()
  app.boot = {
    configured,
    archiveRoot: configured ? 'D:/文档归档' : null,
    sourceRoots: sources,
    counts: EMPTY,
    model: { reachable: false, model: null },
  }
}

describe('引导页（T6.5 / T6.6）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    pickDirectory.mockReset()
    addSourceRoot.mockReset()
    setArchiveRoot.mockReset()
    removeSourceRoot.mockReset()
    scanAll.mockReset()
    replace.mockReset()
    diskFreeGb.mockResolvedValue(120.5)
    vi.spyOn(useAppStore(), 'refreshBoot').mockImplementation(async () => null)
  })

  it('没选源区时「下一步」不可点', async () => {
    setBoot([])
    const w = mount(Onboarding)
    await flushPromises()
    const next = w.findAll('button').find((b) => b.text() === '下一步')
    expect(next?.attributes('disabled')).toBeDefined()
    expect(w.text()).toContain('已选 0 个源区')
  })

  it('选完源区后列表出现，并能走到第二步', async () => {
    setBoot([])
    pickDirectory.mockResolvedValue('C:/Users/kingb/Desktop')
    const app = useAppStore()
    addSourceRoot.mockResolvedValue({ id: 1, path: 'C:/Users/kingb/Desktop', enabled: true })
    app.boot = { ...app.boot!, sourceRoots: [] }
    addSourceRoot.mockImplementation(async () => {
      app.boot = {
        ...app.boot!,
        sourceRoots: [{ id: 1, path: 'C:/Users/kingb/Desktop', enabled: true }],
      }
      return { id: 1, path: 'C:/Users/kingb/Desktop', enabled: true }
    })

    const w = mount(Onboarding)
    await flushPromises()
    await w.findAll('button').find((b) => b.text().includes('选择文件夹'))!.trigger('click')
    await flushPromises()

    expect(w.text()).toContain('C:/Users/kingb/Desktop')
    expect(w.text()).toContain('已选 1 个源区')

    const next = w.findAll('button').find((b) => b.text() === '下一步')
    expect(next?.attributes('disabled')).toBeUndefined()
    await next!.trigger('click')
    expect(w.text()).toContain('选择归档区')
  })

  it('归档区落在源区里 → 按 code 提示「归档区不能放在源区里」', async () => {
    setBoot([{ id: 1, path: 'C:/Users/kingb/Desktop', enabled: true }])
    pickDirectory.mockResolvedValue('C:/Users/kingb/Desktop/归档')
    setArchiveRoot.mockRejectedValue({
      code: 'ARCHIVE_IN_SOURCE',
      message: '归档区不能放在源区里，否则会被重复扫描',
    })

    const w = mount(Onboarding)
    await flushPromises()
    await w.findAll('button').find((b) => b.text() === '下一步')!.trigger('click')
    await w.findAll('button').find((b) => b.text().includes('选择归档区'))!.trigger('click')
    await flushPromises()

    expect(w.find('.err').text()).toContain('归档区不能放在源区里')
    // 停在第 2 步，别把用户甩回去
    expect(w.text()).toContain('选择归档区')
  })

  it('归档区设置成功 → 进入第三步并显示隔离区说明', async () => {
    setBoot([{ id: 1, path: 'C:/Users/kingb/Desktop', enabled: true }])
    pickDirectory.mockResolvedValue('D:/文档归档')
    setArchiveRoot.mockResolvedValue({
      path: 'D:/文档归档',
      freeGb: 300,
      quarantinePath: 'D:/文档归档/.filebutler/quarantine',
    })

    const w = mount(Onboarding)
    await flushPromises()
    await w.findAll('button').find((b) => b.text() === '下一步')!.trigger('click')
    await w.findAll('button').find((b) => b.text().includes('选择归档区'))!.trigger('click')
    await flushPromises()

    expect(w.text()).toContain('D:/文档归档')
    expect(w.text()).toContain('开始扫描')
    expect(w.text()).toContain('去选目录方案')
  })
})
