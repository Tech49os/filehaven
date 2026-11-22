import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { DirMergeCandidate, DirMergeReport, DirMergeStage } from '../../core/types'

const dirMergeScan = vi.fn()
const applyDirMerge = vi.fn()
const undoDirMerge = vi.fn()
let stageCb: ((p: DirMergeStage) => void) | null = null

vi.mock('../../core/ipc', async (importOriginal) => {
  const real = await importOriginal<typeof import('../../core/ipc')>()
  return { ...real, dirMergeScan, applyDirMerge, undoDirMerge }
})
vi.mock('../../core/events', () => ({
  onDirMergeStage: vi.fn().mockImplementation((cb) => {
    stageCb = cb
    return Promise.resolve(() => {})
  }),
}))

const DirMerge = (await import('./DirMerge.vue')).default

function candidate(over: Partial<DirMergeCandidate> = {}): DirMergeCandidate {
  return {
    id: 'dir_merge:sig1',
    signature: 'A/B/2026年/9月',
    dirIds: [1, 2],
    dirPaths: ['A/B/2026年9月', 'A/B/2026年/9月'],
    sourceDirIds: [1],
    sourceDirPaths: ['A/B/2026年9月'],
    targetDirId: 2,
    targetPath: 'A/B/2026年/9月',
    suggestedTargetId: 2,
    reason: '归一化签名=A/B/2026年/9月；差异=2026年9月 vs 2026年/9月',
    fileCount: 4,
    confidence: 0.9,
    source: 'rule',
    ...over,
  }
}

function rep(candidates: DirMergeCandidate[]): DirMergeReport {
  return { candidates, modelReviewed: true, note: '规则初筛 + 模型复核', elapsedMs: 120 }
}

const btn = (w: ReturnType<typeof mount>, label: string) =>
  w.findAll('button').find((b) => b.text().includes(label))

describe('同义目录合并 · 建议列表', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    localStorage.clear()
    dirMergeScan.mockReset()
    applyDirMerge.mockReset().mockResolvedValue({
      migrationId: 12, affectedFiles: 2, renamed: 1, failed: [], sourceDirIds: [1], targetDirId: 2,
    })
    undoDirMerge.mockReset().mockResolvedValue({ restored: 2, skipped: 0, failed: [] })
    stageCb = null
  })

  it('扫描展示候选、理由含归一化签名，默认推荐目标可选并采纳', async () => {
    dirMergeScan.mockResolvedValue(rep([candidate()]))
    const w = mount(DirMerge)
    await btn(w, '同义目录扫描')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('A/B/2026年/9月')
    expect(w.text()).toContain('归一化签名')
    const sel = w.find('select')
    expect(sel.exists()).toBe(true)
    expect((sel.element as HTMLSelectElement).value).toBe('2')
    await btn(w, '采纳')!.trigger('click')
    await flushPromises()
    expect(applyDirMerge).toHaveBeenCalledTimes(1)
    const req = applyDirMerge.mock.calls[0][0]
    expect(req.sourceDirIds).toEqual([1])
    expect(req.targetDirId).toBe(2)
  })

  it('用户可改选目标，源目录随选择重算', async () => {
    dirMergeScan.mockResolvedValue(rep([candidate()]))
    const w = mount(DirMerge)
    await btn(w, '同义目录扫描')!.trigger('click')
    await flushPromises()
    const sel = w.find('select')
    await sel.setValue('1')
    await btn(w, '采纳')!.trigger('click')
    await flushPromises()
    const req = applyDirMerge.mock.calls[0][0]
    expect(req.targetDirId).toBe(1)
    expect(req.sourceDirIds).toEqual([2])
  })

  it('忽略项不进批次，并持久化「不再打扰」', async () => {
    const keep = candidate()
    const ignoreMe = candidate({
      id: 'dir_merge:sig2',
      signature: 'A/B/2026年/10月',
      dirIds: [3, 4],
      dirPaths: ['A/B/2026年10月', 'A/B/2026年/10月'],
      sourceDirIds: [3],
      sourceDirPaths: ['A/B/2026年10月'],
      targetDirId: 4,
      targetPath: 'A/B/2026年/10月',
      suggestedTargetId: 4,
      reason: '归一化签名=A/B/2026年/10月；差异=2026年10月 vs 2026年/10月',
    })
    dirMergeScan.mockResolvedValue(rep([keep, ignoreMe]))
    const w = mount(DirMerge)
    await btn(w, '同义目录扫描')!.trigger('click')
    await flushPromises()

    const ignoreBtns = w.findAll('button').filter((b) => b.text().includes('忽略'))
    await ignoreBtns[1].trigger('click')
    await flushPromises()
    await btn(w, '采纳')!.trigger('click')
    await flushPromises()
    expect(applyDirMerge).toHaveBeenCalledTimes(1)
    expect(applyDirMerge.mock.calls[0][0].sourceDirIds).toEqual([1])
    expect(localStorage.getItem('filebutler.dirMerge.ignored.v1')).toContain('dir_merge:sig2')

    // 重新扫描：被忽略的那一组不再打扰
    dirMergeScan.mockResolvedValue(rep([keep, ignoreMe]))
    await btn(w, '同义目录扫描')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('A/B/2026年/9月')
    expect(w.text()).not.toContain('A/B/2026年/10月')
  })

  it('降级报告显示「未经模型复核」', async () => {
    dirMergeScan.mockResolvedValue({
      ...rep([candidate()]),
      modelReviewed: false,
      note: '模型正忙，未经模型复核',
    })
    const w = mount(DirMerge)
    await btn(w, '同义目录扫描')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('未经模型复核')
  })

  it('采纳后可按批次撤销', async () => {
    dirMergeScan.mockResolvedValue(rep([candidate()]))
    const w = mount(DirMerge)
    await btn(w, '同义目录扫描')!.trigger('click')
    await flushPromises()
    await btn(w, '采纳')!.trigger('click')
    await flushPromises()
    expect(undoDirMerge).not.toHaveBeenCalled()
    await btn(w, '撤销上次合并')!.trigger('click')
    await flushPromises()
    expect(undoDirMerge).toHaveBeenCalledWith(12)
  })
})
