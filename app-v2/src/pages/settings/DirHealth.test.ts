import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { DirHealthReport, DirHealthIssue, DirHealthStage } from '../../core/types'
import tokens from '../../styles/tokens.css?raw'
import dirHealthSource from './DirHealth.vue?raw'

const dirHealthScan = vi.fn()
const applyDirHealth = vi.fn()
const undoDirHealth = vi.fn()
let stageCb: ((p: DirHealthStage) => void) | null = null

vi.mock('../../core/ipc', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../core/ipc')>()
  return { ...real, dirHealthScan, applyDirHealth, undoDirHealth }
})
vi.mock('../../core/events', () => ({
  onDirHealthStage: vi.fn().mockImplementation((cb) => {
    stageCb = cb
    return Promise.resolve(() => {})
  }),
}))

const DirHealth = (await import('./DirHealth.vue')).default

function issue(over: Partial<DirHealthIssue> = {}): DirHealthIssue {
  return {
    id: 'rename:1',
    kind: 'rename',
    dirId: 1,
    dirName: '合同_初稿',
    relPath: '合同_初稿',
    targetPath: '合同',
    reason: '名称含版本/草稿后缀',
    confidence: 0.85,
    affectedFiles: 3,
    source: 'rule',
    ...over,
  }
}

function report(issues: DirHealthIssue[], reviewed = true): DirHealthReport {
  return { issues, modelReviewed: reviewed, note: reviewed ? '规则+模型' : '模型未启动，未经模型复核', elapsedMs: 100 }
}

const btn = (w: ReturnType<typeof mount>, label: string) =>
  w.findAll('button').find((b) => b.text().includes(label))

describe('目录体检 · 可见性', () => {
  it('样式令牌必须在 tokens.css 里定义，别让主按钮白底白字变成隐身', () => {
    const defined = new Set([...tokens.matchAll(/^\s*(--[\w-]+):/gm)].map((m) => m[1]))
    const used = [...new Set([...dirHealthSource.matchAll(/var\((--[\w-]+)/g)].map((m) => m[1]))]
    expect(used.filter((v) => !defined.has(v))).toEqual([])
  })
})

describe('目录体检 · 建议列表', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    dirHealthScan.mockReset()
    applyDirHealth.mockReset().mockResolvedValue({ migrationId: 9, newRelPath: '合同', renamed: false, affectedDirs: 1, affectedFiles: 3 })
    undoDirHealth.mockReset().mockResolvedValue({ restored: 1, skipped: 0, failed: [] })
    stageCb = null
  })

  it('点「目录体检」拉报告并显示建议', async () => {
    dirHealthScan.mockResolvedValue(report([issue()]))
    const w = mount(DirHealth)
    await btn(w, '目录体检')!.trigger('click')
    await flushPromises()
    expect(dirHealthScan).toHaveBeenCalledOnce()
    expect(w.text()).toContain('合同_初稿')
    expect(w.text()).toContain('名称含版本/草稿后缀')
  })

  it('逐条采纳：只把采纳项送进执行批次，忽略项不进入', async () => {
    const keep = issue({ id: 'rename:1', dirId: 1, targetPath: '合同' })
    const ignore = issue({ id: 'rename:2', dirId: 2, dirName: '报告_草稿', relPath: '报告_草稿', targetPath: '报告' })
    dirHealthScan.mockResolvedValue(report([keep, ignore]))
    const w = mount(DirHealth)
    await btn(w, '目录体检')!.trigger('click')
    await flushPromises()

    // 忽略第 2 条
    const ignoreBtns = w.findAll('button').filter((b) => b.text().includes('忽略'))
    await ignoreBtns[1].trigger('click')
    await flushPromises()

    // 采纳第 1 条
    const adoptBtns = w.findAll('button').filter((b) => b.text().includes('采纳'))
    await adoptBtns[0].trigger('click')
    await flushPromises()

    expect(applyDirHealth).toHaveBeenCalledTimes(1)
    const req = applyDirHealth.mock.calls[0][0]
    expect(req.dirId).toBe(1)
    expect(req.issueId).toBe('rename:1')
  })

  it('点「忽略」后该行立即显示已忽略且不再可采纳', async () => {
    dirHealthScan.mockResolvedValue(report([
      issue({ id: 'rename:1', dirId: 1 }),
      issue({ id: 'rename:2', dirId: 2, dirName: '报告_草稿' }),
    ]))
    const w = mount(DirHealth)
    await btn(w, '目录体检')!.trigger('click')
    await flushPromises()

    const ignoreBtns = w.findAll('button').filter((b) => b.text().includes('忽略'))
    await ignoreBtns[0].trigger('click')
    await flushPromises()

    // 响应式反馈：无需其他动作就能看到「已忽略」
    expect(w.text()).toContain('已忽略')
    const firstRow = w.findAll('.issue')[0]
    expect(firstRow.text()).toContain('已忽略')
    expect(firstRow.findAll('button').filter((b) => b.text().includes('采纳'))).toHaveLength(0)
  })

  it('降级报告显示「未经模型复核」标注', async () => {
    dirHealthScan.mockResolvedValue(report([issue()], false))
    const w = mount(DirHealth)
    await btn(w, '目录体检')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('未经模型复核')
  })

  it('进度阶段事件显示当前阶段', async () => {
    dirHealthScan.mockImplementation(async () => {
      stageCb?.({ stage: 'model', elapsedMs: 120, note: '模型复核中' })
      return report([issue()])
    })
    const w = mount(DirHealth)
    await btn(w, '目录体检')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('模型复核中')
  })

  it('empty / merge 建议不提供采纳按钮', async () => {
    dirHealthScan.mockResolvedValue(report([
      issue({ id: 'empty:3', kind: 'empty', targetPath: null, reason: '空目录' }),
      issue({ id: 'merge:4', kind: 'merge', dirName: '资料(1)', targetPath: '资料', reason: '同义' }),
    ]))
    const w = mount(DirHealth)
    await btn(w, '目录体检')!.trigger('click')
    await flushPromises()
    expect(w.findAll('button').filter((b) => b.text().includes('采纳'))).toHaveLength(0)
    expect(w.text()).toContain('需手工确认')
  })
})
