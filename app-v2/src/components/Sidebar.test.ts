import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { BootState } from '../core/types'

const bootState = vi.fn()

vi.mock('../core/ipc', () => ({ bootState }))
vi.mock('vue-router', () => ({
  useRoute: () => ({ path: '/workspace' }),
  useRouter: () => ({ push: vi.fn() }),
}))

const Sidebar = (await import('./Sidebar.vue')).default
const { useAppStore } = await import('../stores/app')

function boot(counts: Partial<BootState['counts']> = {}): BootState {
  return {
    configured: true,
    archiveRoot: 'D:/文档归档',
    sourceRoots: [{ id: 1, path: 'C:/桌面', enabled: true }],
    counts: {
      queued: 0,
      labeled: 0,
      pendingConfirm: 0,
      confirmed: 0,
      migrated: 0,
      parked: 0,
      skipped: 0,
      missing: 0,
      ...counts,
    },
    model: { reachable: true, model: 'qwen2.5:7b' },
  }
}

async function mountWith(counts: Partial<BootState['counts']>) {
  bootState.mockResolvedValue(boot(counts))
  // 侧栏自己不拉数据，读的是全局 store；所以先把 store 灌好再挂
  await useAppStore().refreshBoot()
  const w = mount(Sidebar)
  await flushPromises()
  return w
}

function navText(w: Awaited<ReturnType<typeof mountWith>>, label: string) {
  return w.findAll('.nav').find((n) => n.text().startsWith(label))!.text()
}

describe('侧边栏计数（2026-11-20 反馈：还有多少待整理没入口）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset()
  })

  it('整理台带「还有多少待整理」的角标，数字含排队与已标签的', async () => {
    const w = await mountWith({ queued: 3, labeled: 2, pendingConfirm: 4, confirmed: 6 })
    // 9 = 3 + 2 + 4，一个都不能漏：漏了数字就是错的
    expect(navText(w, '整理台')).toBe('整理台9')
    expect(navText(w, '待迁移')).toBe('待迁移6')
  })

  it('整理台一个不剩时角标显示 0，且不是红色（没活干不该报警）', async () => {
    const w = await mountWith({ confirmed: 2 })
    const row = w.findAll('.nav').find((n) => n.text().startsWith('整理台'))!
    expect(row.find('.badge').text()).toBe('0')
    expect(row.find('.badge').classes()).toContain('zero')
  })

  it('整理台有活时角标不抢注意力（不是待办告警那种红）', async () => {
    const w = await mountWith({ pendingConfirm: 4 })
    const row = w.findAll('.nav').find((n) => n.text().startsWith('整理台'))!
    expect(row.find('.badge').classes()).toContain('soft')
  })
})
