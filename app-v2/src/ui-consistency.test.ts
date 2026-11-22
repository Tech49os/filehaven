import { describe, expect, it } from 'vitest'
import tokens from './styles/tokens.css?raw'
import fileCard from './components/FileCard.vue?raw'
import parked from './pages/parked/Parked.vue?raw'
import migration from './pages/migration/PendingMigration.vue?raw'
import mainSource from './main.ts?raw'

/** 全局共享样式表（BUG-008 的按钮基线 + 文件条目外壳） */
const styles = import.meta.glob<string>('./styles/*.css', {
  query: '?raw',
  import: 'default',
  eager: true,
})
const shared = styles['./styles/shared.css']

/** src/components + src/pages 下所有组件源码，用来做「裸 .btn 定义」静态检查 */
const components = {
  ...import.meta.glob<string>('./components/*.vue', {
    query: '?raw',
    import: 'default',
    eager: true,
  }),
  ...import.meta.glob<string>('./pages/**/*.vue', {
    query: '?raw',
    import: 'default',
    eager: true,
  }),
}

function tokenMap(css: string) {
  return new Map(
    [...css.matchAll(/^\s*(--[\w-]+):\s*([^;]+);/gm)].map((m) => [m[1], m[2].trim()]),
  )
}

describe('BUG-008 · 文件条目尺度 token', () => {
  it('tokens.css 定义固定名字的 token，且达到可读基线', () => {
    const defs = tokenMap(tokens)
    const pxMin: Record<string, number> = {
      '--fs-file-name': 13,
      '--fs-meta': 12,
      '--fs-chip': 11,
    }
    for (const [name, min] of Object.entries(pxMin)) {
      const value = defs.get(name)
      expect(value, `${name} 必须在 tokens.css 定义`).toBeTruthy()
      expect(parseFloat(value!), `${name} 必须 ≥ ${min}px`).toBeGreaterThanOrEqual(min)
    }
    expect(parseFloat(defs.get('--lh-file') ?? '0')).toBeGreaterThanOrEqual(1.6)
    for (const name of ['--pad-row-x', '--pad-row-y', '--gap-row', '--r-item']) {
      expect(defs.get(name), `${name} 必须在 tokens.css 定义`).toBeTruthy()
    }
  })

  it('整理台 / 待定区 / 待迁移三处文件条目共用 .file-entry 外壳类', () => {
    expect(fileCard).toMatch(/class="card file-entry"/)
    expect(parked).toMatch(/class="row file-entry"/)
    expect(migration).toMatch(/class="frow file-entry"/)
  })

  it('共享样式表定义统一尺度并在入口引入', () => {
    expect(shared, '必须存在 src/styles/shared.css').toBeTruthy()
    expect(mainSource, 'main.ts 必须引入 shared.css').toContain("import './styles/shared.css'")
    for (const name of [
      '--fs-file-name',
      '--fs-meta',
      '--fs-chip',
      '--lh-file',
      '--pad-row-x',
      '--pad-row-y',
      '--gap-row',
      '--r-item',
      '--c-surface',
    ]) {
      expect(shared, `shared.css 必须用 ${name}`).toContain(`var(${name})`)
    }
  })

  it('待定区从灰底大行改为规范条目（底色不再是 --c-field）', () => {
    expect(parked).not.toMatch(/\.row\s*\{[^}]*background:\s*var\(--c-field\)/)
    expect(shared).toMatch(/\.file-entry\s*\{[^}]*background:\s*var\(--c-surface\)/)
  })
})

describe('BUG-008 · .btn 收敛', () => {
  it('只有一份全局 .btn 基线定义', () => {
    expect(shared).toBeTruthy()
    const bare = [...shared!.matchAll(/^\s*\.btn\s*\{/gm)]
    expect(bare, '全局样式表里应恰有一份裸 .btn 定义').toHaveLength(1)
  })

  it('组件 / 页面里不再有裸 .btn 定义，只留场景特化覆盖', () => {
    const offenders = Object.entries(components)
      .filter(([, src]) => /^\s*\.btn\s*\{/m.test(src))
      .map(([path]) => path)
    expect(offenders).toEqual([])
  })
})
