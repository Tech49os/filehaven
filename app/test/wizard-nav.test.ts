/**
 * 向导跳转的静态体检。
 *
 * 起因：ReviewStep 的「预览迁移计划」按钮写成 `w.goto(5)`——它自己就是第 5 步，
 * 点下去毫无反应，用户只看到「功能不可用」。这种错没有类型错误、没有单测覆盖，
 * 只能靠人眼发现，而人眼显然漏了。
 *
 * 这里不挂载组件（项目没有 vue-test-utils），直接读 .vue 源码做结构检查：
 *  1. 向导的步骤表必须是 1..8 连续，中途不能跳号
 *  2. 组件里任何 `goto(n)` 都不许指向「它自己所在的步骤」——那是死按钮
 *  3. 每个步骤组件都必须真的能到达（被 WizardPage 引用）
 */
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const here = dirname(fileURLToPath(import.meta.url))
const pagesDir = resolve(here, '../src/pages/wizard')

const read = (f: string) => readFileSync(join(pagesDir, f), 'utf8')

/** 注释里也会出现 goto(5) 这种字样（比如解释历史 bug），扫描前先去掉 */
const stripComments = (src: string) =>
  src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1')

/** WizardPage 里的步骤 → 组件映射（数组下标就是步骤号） */
function stepMap(): string[] {
  const src = read('WizardPage.vue')
  const block = src.match(/const map = \[([\s\S]*?)\]/)
  if (!block) throw new Error('WizardPage.vue 里找不到步骤表')
  return block[1]
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

/** 组件源码里找它代表第几步（同一组件可挂多个步骤，如 MigrateStep） */
function stepsOf(component: string, map: string[]): number[] {
  return map.map((c, i) => (c === component ? i : -1)).filter((i) => i > 0)
}

const files = readdirSync(pagesDir).filter((f) => f.endsWith('.vue'))

describe('向导步骤表', () => {
  it('步骤表必须是 1..8，下标 0 是占位', () => {
    const map = stepMap()
    expect(map.length).toBe(9)
    expect(map[0]).toBe(map[1])
  })

  it('每一步都有真实组件（不是占位空串）', () => {
    for (const c of stepMap()) expect(c).toMatch(/^\w+$/)
  })
})

describe('跳转不能是死按钮', () => {
  for (const f of files) {
    const src = stripComments(read(f))
    const gotos = [...src.matchAll(/\.goto\((\d+)\)/g)].map((m) => Number(m[1]))
    if (gotos.length === 0) continue

    it(`${f} 的 goto 都指向别的步骤`, () => {
      const mine = stepsOf(f.replace('.vue', ''), stepMap())
      if (mine.length === 0) return // 不在步骤表里（如子组件）就不管
      for (const n of gotos) {
        expect(n).toBeGreaterThanOrEqual(1)
        expect(n).toBeLessThanOrEqual(8)
        // MigrateStep 同时挂在 6、7 两步，goto(7) 是它的正常自跳，放行
        if (mine.length === 1) expect(mine).not.toContain(n)
      }
    })
  }
})

describe('预览迁移计划真的能到', () => {
  it('第 6 步是 MigrateStep', () => {
    expect(stepMap()[6]).toBe('MigrateStep')
  })

  it('ReviewStep（第 5 步）有一个按钮把它推到第 6 步', () => {
    expect(read('ReviewStep.vue')).toMatch(/\.goto\(6\)/)
  })
})
