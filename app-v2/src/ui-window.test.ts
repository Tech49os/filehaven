/**
 * 块① Finder 式窗口框架（红→绿）
 *
 * 源码级契约 + 组件挂载断言：窗口灯、拖拽热区、尺寸 token、
 * Sidebar 材质归属、检索胶囊、掩膜 alpha。真机 CDP 量测不在这里。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import tokens from './styles/tokens.css?raw'
import appWindow from './components/AppWindow.vue?raw'
import sidebar from './components/Sidebar.vue?raw'
import appSource from './App.vue?raw'
import glassCss from './styles/glass.css?raw'

/** getCurrentWindow() 的三个窗口操作，用 hoisted 保证 mock 工厂拿得到 */
const winMocks = vi.hoisted(() => ({
  close: vi.fn(),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
}))

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => winMocks }))

// App.vue 在「检索胶囊」用例里只验证 slot 与点击派发，依赖全部给最小实现
vi.mock('vue-router', () => ({
  useRoute: () => ({ meta: {}, path: '/workspace' }),
  useRouter: () => ({ replace: vi.fn() }),
}))
vi.mock('./stores/app', () => ({
  useAppStore: () => ({ ready: true, configured: true, error: '', refreshBoot: vi.fn() }),
}))
vi.mock('./stores/ui', () => ({
  useUiStore: () => ({ glassEnabled: true }),
}))

const AppWindow = (await import('./components/AppWindow.vue')).default
const App = (await import('./App.vue')).default

function tokenMap(css: string) {
  return new Map(
    [...css.matchAll(/^\s*(--[\w-]+):\s*([^;]+);/gm)].map((m) => [m[1], m[2].trim()]),
  )
}

/** AppWindow 的 isTauri 守卫在 setup 时读这个全局，挂载前先放好 */
function fakeTauri() {
  ;(window as unknown as { __TAURI_INTERNALS__?: Record<string, unknown> }).__TAURI_INTERNALS__ = {}
}

async function mountWindow(withSide = true) {
  return mount(AppWindow, {
    slots: {
      ...(withSide ? { side: '<div class="fake-side" />' } : {}),
      tail: '<button class="fake-tail" />',
      default: '<div class="fake-body" />',
    },
  })
}

describe('块① · 窗口灯与右上角三键', () => {
  beforeEach(() => {
    fakeTauri()
    winMocks.close.mockClear()
    winMocks.minimize.mockClear()
    winMocks.toggleMaximize.mockClear()
  })

  it('三个灯是 button，分别调用 close / minimize / toggleMaximize', async () => {
    expect(appWindow).not.toMatch(/winbtns/)
    const w = await mountWindow()
    expect(w.find('.winbtns').exists()).toBe(false)

    const lamps = w.findAll('.lamp')
    expect(lamps).toHaveLength(3)
    expect(lamps.map((l) => l.attributes('aria-label'))).toEqual([
      '关闭窗口',
      '最小化窗口',
      '最大化或还原窗口',
    ])
    expect(lamps.map((l) => l.attributes('title'))).toEqual(['关闭', '最小化', '最大化/还原'])

    await lamps[0].trigger('click')
    expect(winMocks.close).toHaveBeenCalledTimes(1)
    await lamps[1].trigger('click')
    expect(winMocks.minimize).toHaveBeenCalledTimes(1)
    await lamps[2].trigger('click')
    expect(winMocks.toggleMaximize).toHaveBeenCalledTimes(1)
  })

  it('rail 与标题栏是拖拽区，灯按钮本身不是', async () => {
    const w = await mountWindow()
    expect(w.find('.side-rail').attributes('data-tauri-drag-region')).not.toBeUndefined()
    expect(w.find('.titlebar').attributes('data-tauri-drag-region')).not.toBeUndefined()
    for (const lamp of w.findAll('.lamp')) {
      expect(lamp.attributes('data-tauri-drag-region')).toBeUndefined()
    }
  })

  it('showSide 缺省时 rail 不渲染（用 $slots.side 判断）', async () => {
    const w = await mountWindow(false)
    expect(w.find('.side-rail').exists()).toBe(false)
    expect(w.find('.titlebar').exists()).toBe(true)
  })
})

describe('块① · 尺寸与材质归属', () => {
  it('rail 宽取 --side-width(224px)，标题栏高取 --titlebar-h(52px)', () => {
    const defs = tokenMap(tokens)
    expect(defs.get('--side-width')).toBe('224px')
    expect(defs.get('--titlebar-h')).toBe('52px')
    expect(appWindow).toMatch(/var\(--side-width\)/)
    expect(appWindow).toMatch(/var\(--titlebar-h\)/)
  })

  it('Sidebar 根不再自带 glass-side 与固定宽度/媒体查询', () => {
    expect(sidebar).toMatch(/<div class="side">/)
    expect(sidebar).not.toMatch(/glass-side/)
    expect(sidebar).not.toMatch(/@media/)
    // 只查根 .side 块：其余 width 是角标/状态点，不在禁用范围
    const rootBlock = sidebar.match(/\.side\s*\{([^}]*)\}/)?.[1] ?? ''
    expect(rootBlock).not.toMatch(/width/)
  })

  it('rail 唯一挂 glass-side，标题栏挂 glass-title，主面板由 App.vue 挂', () => {
    expect(appWindow).toMatch(/class="side-rail glass-side"/)
    expect(appWindow).toMatch(/class="titlebar glass-title"/)
    expect(appSource).toMatch(/class="main glass-panel"/)
    expect(appWindow).not.toMatch(/class="body glass-panel"/)
  })
})

describe('块① · 快速检索胶囊与掩膜 token', () => {
  it('胶囊可见可点，文案含 Ctrl K，点击派发 Ctrl+K 键盘事件', async () => {
    expect(appSource).toMatch(/#tail/)
    const RouterViewStub = {
      template: '<div class="router-view-stub"><slot :Component="Page" /></div>',
      setup() {
        return { Page: { template: '<div class="page-stub" />' } }
      },
    }
    const w = mount(App, {
      global: {
        stubs: {
          Sidebar: true,
          SearchPanel: true,
          RouterView: RouterViewStub,
        },
      },
    })
    await flushPromises()

    const pill = w.find('.quick-search')
    expect(pill.exists()).toBe(true)
    expect(pill.text()).toContain('Ctrl K')

    const seen: KeyboardEvent[] = []
    const spy = (e: KeyboardEvent) => seen.push(e)
    window.addEventListener('keydown', spy)
    await pill.trigger('click')
    window.removeEventListener('keydown', spy)
    expect(seen.some((e) => e.ctrlKey && e.key.toLowerCase() === 'k')).toBe(true)
  })

  it('--c-mask 已定义且 alpha 在 0.14~0.18', () => {
    const mask = tokenMap(tokens).get('--c-mask')
    expect(mask, '--c-mask 必须在 tokens.css 定义').toBeTruthy()
    const m = mask!.match(/rgba\(\s*[\d.]+\s*,\s*[\d.]+\s*,\s*[\d.]+\s*,\s*([\d.]+)\s*\)/)
    expect(m, '--c-mask 必须是 rgba(...)').toBeTruthy()
    const alpha = parseFloat(m![1])
    expect(alpha).toBeGreaterThanOrEqual(0.14)
    expect(alpha).toBeLessThanOrEqual(0.18)
  })

  it('侧栏无竖向分割亮线：材质态与实色回退态都不带 border-right', () => {
    // design §2：侧栏从窗口顶贯通到底，「无断层、无竖向分割亮线」。
    // BUG-011 遗留的 border-right 会让贯通侧栏右缘出现一条白线，两处回退都要去掉。
    const blocks = [
      ...glassCss.matchAll(/\.glass-side\s*\{([^}]*)\}/g),
      ...glassCss.matchAll(/html\.no-glass\s+\.glass-side\s*\{([^}]*)\}/g),
    ]
    expect(blocks.length, 'glass.css 应含材质态与实色回退态两条 .glass-side 规则').toBeGreaterThanOrEqual(2)
    for (const [, body] of blocks) {
      expect(body, '.glass-side 不应有 border-right（竖向分割亮线）').not.toMatch(/border-right\s*:/)
    }
  })
})
