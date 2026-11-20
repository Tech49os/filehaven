import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const bootState = vi.fn()
const setWatchEnabled = vi.fn()
vi.mock('../core/ipc', () => ({ bootState, setWatchEnabled }))

const { useAppStore } = await import('./app')

function state(confirmed: number, parked = 0, configured = true) {
  return {
    configured,
    archiveRoot: 'D:/文档归档',
    sourceRoots: [{ id: 1, path: 'C:/Users/kingb/Desktop', enabled: true }],
    counts: {
      queued: 0,
      labeled: 2,
      pendingConfirm: 10,
      confirmed,
      migrated: 0,
      parked,
      skipped: 3,
      missing: 0,
    },
    model: { reachable: true, model: 'qwen2.5:7b' },
    watchEnabled: true,
  }
}

describe('app store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset()
  })

  it('refreshBoot 两次后 counts.confirmed 变成新值（每个写操作后都要刷新）', async () => {
    bootState.mockResolvedValueOnce(state(3)).mockResolvedValueOnce(state(5))
    const s = useAppStore()
    await s.refreshBoot()
    expect(s.counts.confirmed).toBe(3)
    await s.refreshBoot()
    expect(s.counts.confirmed).toBe(5)
  })

  it('侧边栏角标：待迁移 = confirmed，待定区 = parked', async () => {
    bootState.mockResolvedValueOnce(state(7, 4))
    const s = useAppStore()
    await s.refreshBoot()
    expect(s.pendingMigration).toBe(7)
    expect(s.parkedCount).toBe(4)
    expect(s.pendingTotal).toBe(12) // labeled 2 + pendingConfirm 10
  })

  it('未配置时 configured=false（用于决定首屏跳引导）', async () => {
    bootState.mockResolvedValueOnce({
      ...state(0),
      configured: false,
      archiveRoot: null,
      sourceRoots: [],
    })
    const s = useAppStore()
    await s.refreshBoot()
    expect(s.configured).toBe(false)
  })

  it('后端报错不抛到页面，落进 error 且 counts 保持上次数值', async () => {
    bootState.mockResolvedValueOnce(state(2))
    const s = useAppStore()
    await s.refreshBoot()
    bootState.mockRejectedValueOnce({ code: 'DB', message: '数据库锁已损坏' })
    await s.refreshBoot()
    expect(s.error).toBe('数据库锁已损坏')
    expect(s.counts.confirmed).toBe(2)
  })

  it('默认刷新不探模型（判定后每次都会调，探活是同步 HTTP 会卡手）', async () => {
    bootState.mockResolvedValue(state(1))
    const s = useAppStore()
    await s.refreshBoot()
    await s.refreshBoot()
    expect(bootState.mock.calls[0]).toEqual([false])
    expect(bootState.mock.calls[1]).toEqual([false])
  })

  it('启动首调带 probe，真去问一次模型', async () => {
    bootState.mockResolvedValueOnce(state(1))
    const s = useAppStore()
    await s.refreshBoot({ probe: true })
    expect(bootState.mock.calls[0]).toEqual([true])
  })

  // 后端 Counts 是 #[serde(rename_all = "camelCase")]：线上字段名是 pendingConfirm。
  // 前端一度照 Rust 的蛇形名写，取到 undefined，侧栏角标直接显示「NaN」——
  // 这类错编译器不管，只有喂一份真实线上形状的 counts 才抓得到。
  it('按后端真实字段名（camelCase）算出角标，不能是 NaN', async () => {
    bootState.mockResolvedValueOnce(state(2, 1))
    const s = useAppStore()
    await s.refreshBoot()
    expect(Number.isNaN(s.pendingTotal)).toBe(false)
    expect(s.pendingTotal).toBe(12)
  })
})

// ------------------------------------------------ 新文件提示 + 监听开关（P2 T3.7）

describe('app store · 新文件与监听', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset()
    setWatchEnabled.mockReset()
    vi.useRealTimers()
  })

  it('两批新文件累加（后一批不能把前一批盖掉）', () => {
    const s = useAppStore()
    s.noteNewFiles(3)
    s.noteNewFiles(2)
    expect(s.newFiles).toBe(5)
  })

  it('30 秒后自动清零 —— 陈旧数字挂在侧栏比不提示更烦', () => {
    vi.useFakeTimers()
    const s = useAppStore()
    s.noteNewFiles(4)
    expect(s.newFiles).toBe(4)
    vi.advanceTimersByTime(29_000)
    expect(s.newFiles).toBe(4)
    vi.advanceTimersByTime(1_500)
    expect(s.newFiles).toBe(0)
  })

  it('点掉提示立即清零，且不再被旧定时器改回去', () => {
    vi.useFakeTimers()
    const s = useAppStore()
    s.noteNewFiles(4)
    s.dismissNewFiles()
    expect(s.newFiles).toBe(0)
    vi.advanceTimersByTime(60_000)
    expect(s.newFiles).toBe(0)
  })

  it('added=0 不产生提示（扫了一圈没新东西就别吱声）', () => {
    const s = useAppStore()
    s.noteNewFiles(0)
    s.noteNewFiles(-2)
    expect(s.newFiles).toBe(0)
  })

  it('开关监听会写后端并刷新启动态（界面要跟上真实状态）', async () => {
    bootState.mockResolvedValue({ ...state(0), watchEnabled: false })
    setWatchEnabled.mockResolvedValue(undefined)
    const s = useAppStore()
    await s.setWatchEnabled(false)
    expect(setWatchEnabled).toHaveBeenCalledWith(false)
    expect(s.watchEnabled).toBe(false)
  })

  it('没拿到 watchEnabled 时按「开着」算（默认行为，不能误报成关闭）', () => {
    const s = useAppStore()
    expect(s.watchEnabled).toBe(true)
  })
})
