/**
 * BUG-011 · 全局毛玻璃材质（红→绿）
 *
 * 全部是「源码级」断言：token 表 / 组件挂载点 / 回退分支 / 开关持久化。
 * 真正的视觉量测（backdropFilter、alpha、对比度、三档截图）走 CDP，见
 * bugfix/BUG-011-修复说明.md；这里先把「必须存在」的契约钉死。
 */
import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import tokens from './styles/tokens.css?raw'
import glass from './styles/glass.css?raw'
import appWindow from './components/AppWindow.vue?raw'
import sidebar from './components/Sidebar.vue?raw'
import appSource from './App.vue?raw'
import searchPanel from './components/SearchPanel.vue?raw'
import settings from './pages/settings/Settings.vue?raw'

function tokenMap(css: string) {
  return new Map(
    [...css.matchAll(/^\s*(--[\w-]+):\s*([^;]+);/gm)].map((m) => [m[1], m[2].trim()]),
  )
}

describe('BUG-011 · 材质 token 集中定义', () => {
  it('tokens.css 定义四层模糊半径与半透明底色', () => {
    const defs = tokenMap(tokens)
    const required = [
      '--glass-blur-title',
      '--glass-blur-side',
      '--glass-blur-panel',
      '--glass-blur-float',
      '--glass-bg-title',
      '--glass-bg-side',
      '--glass-bg-panel',
      '--glass-bg-float',
      '--glass-border',
      '--glass-saturate',
    ]
    for (const name of required) {
      expect(defs.get(name), `${name} 必须在 tokens.css 定义`).toBeTruthy()
    }
    const radii = ['--glass-blur-title', '--glass-blur-side', '--glass-blur-panel', '--glass-blur-float'].map(
      (n) => parseFloat(defs.get(n)!),
    )
    expect(new Set(radii).size, '至少两处模糊半径不同，形成层次').toBeGreaterThanOrEqual(2)
    for (const n of ['--glass-bg-title', '--glass-bg-side', '--glass-bg-panel']) {
      expect(defs.get(n), `${n} 必须是半透明（alpha<1）`).toMatch(/rgba\([^)]*,\s*0?\.\d+\)/)
    }
  })

  it('旧「为性能放弃毛玻璃」决策注释已删除', () => {
    expect(tokens).not.toMatch(/放弃毛玻璃|为性能|纯浪费帧/)
  })
})

describe('BUG-011 · 组件真实挂载材质', () => {
  it('.win 不再用不透明 --c-bg 覆盖底层，标题栏挂材质类', () => {
    expect(appWindow).not.toMatch(/\.win\s*\{[^}]*background:\s*var\(--c-bg\)/)
    expect(appWindow).toMatch(/class="win glass-frame"/)
    expect(appWindow).toMatch(/class="titlebar glass-title"/)
  })

  it('侧栏材质交由父级 rail；主面板 / 浮层分别挂上材质类', () => {
    // 块①：Sidebar 退成内部容器，.glass-side 只由 AppWindow 的 .side-rail 提供
    expect(sidebar).toMatch(/<div class="side">/)
    expect(sidebar).not.toMatch(/glass-side/)
    expect(appWindow).toMatch(/class="side-rail glass-side"/)
    expect(appSource).toMatch(/class="main glass-panel"/)
    expect(searchPanel).toMatch(/class="panel glass-float"/)
  })

  it('浮层文字有 .glass-text 底衬', () => {
    expect(searchPanel).toMatch(/class="[^"]*glass-text/)
  })

  it('材质不接到列表每一行', () => {
    expect(glass).not.toMatch(/\.file-entry\s*\{[^}]*backdrop-filter/)
    expect(glass).not.toMatch(/\.list-surface\s*\{[^}]*backdrop-filter/)
    expect(glass).toMatch(/列表[^]*?不用毛玻璃|不用毛玻璃/)
  })
})

describe('BUG-011 · glass.css 材质与实色回退', () => {
  it('四层框架都带 backdrop-filter: blur(...) saturate(...)', () => {
    for (const cls of ['glass-title', 'glass-side', 'glass-panel', 'glass-float']) {
      expect(glass, `${cls} 必须有 backdrop-filter blur + saturate`).toMatch(
        new RegExp(`\\.${cls}\\s*\\{[^}]*backdrop-filter:\\s*blur\\([^;]*saturate\\(`),
      )
    }
  })

  it('关闭开关（html.no-glass）时完整回退实色', () => {
    expect(glass).toMatch(/html\.no-glass[\s\S]*?backdrop-filter:\s*none/)
    expect(glass).toMatch(/html\.no-glass[\s\S]*?background:\s*var\(--c-titlebar\)/)
    expect(glass).toMatch(/html\.no-glass[\s\S]*?background:\s*var\(--c-sidebar\)/)
  })

  it('@supports not (backdrop-filter) 时回退实色', () => {
    expect(glass).toMatch(/@supports not \([\s\S]*?backdrop-filter[\s\S]*?\)/)
    expect(glass).toMatch(/@supports not \([\s\S]*?background:\s*var\(--c-(titlebar|sidebar|bg)\)/)
  })
})

describe('BUG-011 · 材质开关（默认开 + 持久化）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    localStorage.clear()
  })

  it('默认开启；写盘后新建 store 仍读回关闭态', async () => {
    const { useUiStore } = await import('./stores/ui')
    const s = useUiStore()
    expect(s.glassEnabled).toBe(true)
    s.setGlassEnabled(false)
    expect(s.glassEnabled).toBe(false)
    expect(localStorage.getItem('filebutler.glassEnabled')).toBe('0')
    setActivePinia(createPinia())
    expect(useUiStore().glassEnabled).toBe(false)
  })

  it('设置页有「材质效果」开关', () => {
    expect(settings).toMatch(/材质效果/)
    expect(settings).toMatch(/setGlassEnabled/)
    expect(settings).toMatch(/glassEnabled/)
  })
})
