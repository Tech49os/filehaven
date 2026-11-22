import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import settingsSource from './Settings.vue?raw'
import type { DirNode } from '../../core/types'

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
const cleanBadTags = vi.fn()

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
  cleanBadTags,
  createDir: vi.fn(),
  previewDirName: vi.fn(),
  dirHealthScan: vi.fn().mockResolvedValue({ issues: [], modelReviewed: true, note: '', elapsedMs: 0 }),
  applyDirHealth: vi.fn(),
  undoDirHealth: vi.fn(),
}))
vi.mock('../../core/events', () => ({
  onScanProgress: vi.fn().mockResolvedValue(() => {}),
  onDeepTagProgress: vi.fn().mockResolvedValue(() => {}),
  onFilesFound: vi.fn().mockResolvedValue(() => {}),
  onDirHealthStage: vi.fn().mockResolvedValue(() => {}),
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

  it('目录体检有自己的首屏入口卡片，不能埋在目录树卡片底部', async () => {
    const w = mount(Settings)
    await flushPromises()
    const cards = w.findAll('section.card')
    const health = cards.find((c) => c.text().includes('目录体检'))
    expect(health, '目录体检应该有独立卡片，而不是拖在目录树末尾').toBeTruthy()
    // 必须第一张卡：设置页多张卡会互相压缩，放在后面依然要滚动才能看到
    expect(cards[0].text()).toContain('目录体检')
    expect(health!.findAll('button').some((b) => b.text().includes('目录体检'))).toBe(true)
    const tree = cards.find((c) => c.text().includes('AI 建议目录'))
    expect(tree?.findAll('button').some((b) => b.text().includes('目录体检'))).toBe(false)
  })

  // BUG-002：设置页与浏览页共用同一默认折叠策略（helper），第 3 层默认折叠
  it('目录树默认折到第 2 层：第 3 层带子节点的目录 aria-expanded=false', async () => {
    const node = (id: number, depth: number, children: DirNode[] = []): DirNode => ({
      id, name: `目录${id}`, relPath: `目录${id}`, depth,
      origin: 'user', fileCount: 0, children,
    })
    listDirs.mockResolvedValue([
      node(1, 1, [node(2, 2, [node(3, 3, [node(4, 4)])])]),
    ])
    const w = mount(Settings)
    await flushPromises()

    const treeitems = w.findAll('[role="treeitem"][aria-expanded]')
    const expanded1 = treeitems.filter((n) => n.attributes('aria-expanded') === 'true')
    const collapsed3 = treeitems.find(
      (n) => n.text().includes('目录3') && n.attributes('aria-expanded') === 'false',
    )
    expect(expanded1.some((n) => n.text().includes('目录1'))).toBe(true)
    expect(expanded1.some((n) => n.text().includes('目录2'))).toBe(true)
    expect(collapsed3, '第 3 层应默认折叠').toBeTruthy()
    // 第 4 层整行不渲染
    expect(w.text()).not.toContain('目录4')
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
    cleanBadTags.mockReset()
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

  it('BUG-004：标签词表提供「清理弱标签」入口，调用后刷新列表', async () => {
    listTags.mockResolvedValue([
      { id: 11, name: '务要', kind: 'keyword', fileCount: 1 },
      { id: 12, name: '服务委托书', kind: 'topic', fileCount: 1 },
    ])
    cleanBadTags.mockResolvedValue(2)
    const w = mount(Settings)
    await flushPromises()

    const btnClean = btn(w, '清理弱标签')
    expect(btnClean).toBeTruthy()
    await btnClean!.trigger('click')
    await flushPromises()

    expect(cleanBadTags).toHaveBeenCalledTimes(1)
    // 清理命令跑完必须重拉词表，脏标签才会从界面消失
    expect(listTags).toHaveBeenCalledTimes(2)
    expect(w.text()).toContain('已清理 2')
  })
})

// ------------------------------------------------ 图片解读开关（P3 T4.6）

describe('设置页 · 图片解读开关', () => {
  const stats = (over: Record<string, unknown> = {}) => ({
    done: 12,
    failed: 0,
    remaining: 30,
    current: '',
    running: false,
    paused: false,
    enabled: true,
    note: null,
    visionDone: 5,
    visionSkipped: 2,
    visionEnabled: true,
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

  it('开关写库：点「关掉」传 visionEnabled=false，点「打开」传 true', async () => {
    const w = mount(Settings)
    await flushPromises()

    setDeepTag.mockResolvedValueOnce(stats({ visionEnabled: false }))
    await btn(w, '关掉')!.trigger('click')
    await flushPromises()
    expect(setDeepTag).toHaveBeenCalledWith({ visionEnabled: false })
    expect(w.text()).toContain('（已关）')

    setDeepTag.mockResolvedValueOnce(stats({ visionEnabled: true }))
    await btn(w, '打开')!.trigger('click')
    await flushPromises()
    expect(setDeepTag).toHaveBeenCalledWith({ visionEnabled: true })
  })

  it('统计数字：已读/跳过张数亮出来，不藏起来', async () => {
    deepTagStatus.mockResolvedValueOnce(stats({ visionDone: 7, visionSkipped: 3 }))
    const w = mount(Settings)
    await flushPromises()
    expect(w.text()).toContain('已读 7 张')
    expect(w.text()).toContain('跳过 3 张')
  })

  it('模型未响应时把 note 原样亮出来，不静默', async () => {
    deepTagStatus.mockResolvedValueOnce(stats({ note: '队列已停：模型未响应' }))
    const w = mount(Settings)
    await flushPromises()
    expect(w.text()).toContain('队列已停：模型未响应')
  })
})

// ------------------------------------------------ BUG-010 设置页抗压缩 + 目录独立滚动

describe('设置页 · BUG-010 卡片抗压缩与目录树独立滚动', () => {
  /** 4 层 × 每层 3 子节点 = 160 个节点，远超验收要求的 100+ */
  function bigTree(): DirNode[] {
    let id = 1
    const make = (depth: number, idx: number): DirNode => ({
      id: id++,
      name: `d${depth}-${idx}`,
      relPath: `/d${depth}-${idx}`,
      depth,
      origin: 'imported',
      fileCount: 0,
      children:
        depth < 4
          ? [make(depth + 1, idx * 3 + 1), make(depth + 1, idx * 3 + 2), make(depth + 1, idx * 3 + 3)]
          : [],
    })
    return [1, 2, 3, 4].map((i) => make(1, i))
  }

  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset().mockResolvedValue(boot([{ id: 1, path: 'C:/桌面', enabled: true }]))
    listDirs.mockReset().mockResolvedValue(bigTree())
    diskFreeGb.mockReset().mockResolvedValue(120.5)
    listTags.mockReset().mockResolvedValue([])
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
    push.mockReset()
  })

  it('红：每张卡片带抗压缩 class，且 .grid > .card 用 flex:0 0 auto 不被压扁', async () => {
    const w = mount(Settings)
    await flushPromises()
    const cards = w.findAll('section.card')
    expect(cards.length, '设置页至少 7 张卡（BUG-003 拆出体检卡后为 8 张）').toBeGreaterThanOrEqual(7)
    for (const c of cards) {
      expect(c.classes(), '每张卡都要带 settings-card 抗压缩 class').toContain('settings-card')
    }
    expect(settingsSource, '必须存在 .grid > .card 规则').toContain('.grid > .card')
    const gridRule = settingsSource.slice(
      settingsSource.indexOf('.grid > .card'),
      settingsSource.indexOf('.grid > .card') + 200,
    )
    expect(gridRule, '必须在 .grid > .card 上写 flex:0 0 auto').toContain('flex: 0 0 auto')
  })

  it('红：目录树有独立滚动容器，max-height/overflow/min-height 只加在容器上', async () => {
    const w = mount(Settings)
    await flushPromises()
    expect(w.find('.tree-scroll').exists(), '目录树必须包在独立滚动容器 .tree-scroll 里').toBe(true)
    expect(settingsSource, '必须存在 .tree-scroll 规则').toContain('.tree-scroll')
    const scrollRule = settingsSource.slice(
      settingsSource.indexOf('.tree-scroll'),
      settingsSource.indexOf('.tree-scroll') + 200,
    )
    expect(scrollRule, '.tree-scroll 必须限制 max-height').toContain('max-height: min(360px, 40vh)')
    expect(scrollRule, '.tree-scroll 必须可滚动').toContain('overflow: auto')
    expect(scrollRule, 'min-height:0 只加在真正的滚动容器上').toContain('min-height: 0')
  })

  it('红：目录树加载后第 3 层 id 进 collapsed，其子节点（第 4 层）不渲染', async () => {
    const w = mount(Settings)
    await flushPromises()
    const names = w.findAll('.row .name').map((n) => n.text())
    expect(names.some((n) => n.startsWith('d1-')), '第 1 层应渲染').toBe(true)
    expect(names.some((n) => n.startsWith('d2-')), '第 2 层应渲染').toBe(true)
    expect(names.some((n) => n.startsWith('d3-')), '第 3 层节点本身仍可见（其子节点被折叠）').toBe(true)
    expect(names.some((n) => n.startsWith('d4-')), '第 4 层应因第 3 层被折叠而不渲染').toBe(false)
    const d3Rows = w
      .findAll('.row')
      .filter((r) => r.find('.name').exists() && r.find('.name').text().startsWith('d3-'))
    expect(d3Rows.length).toBeGreaterThan(0)
    expect(d3Rows.every((r) => r.attributes('aria-expanded') === 'false'), '第 3 层节点都应处于收起态').toBe(true)
  })

  it('红：100+ 节点时卡片数量与抗压缩结构不退化（真实几何由 CDP 量测）', async () => {
    const w = mount(Settings)
    await flushPromises()
    const cards = w.findAll('section.card')
    expect(cards.length).toBeGreaterThanOrEqual(7)
    expect(w.findAll('section.card.settings-card').length, '160 节点下每张卡都要抗压缩').toBe(cards.length)
    expect(w.find('.tree-scroll').exists()).toBe(true)
    // 体检入口仍是第一张卡（BUG-003 不可破坏）
    expect(w.findAll('section.card')[0].text()).toContain('目录体检')
  })
})
