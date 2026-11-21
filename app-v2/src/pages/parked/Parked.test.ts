import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import type { DeepTagStats, FileRow, ParkedItem } from '../../core/types'

const bootState = vi.fn()
const listParked = vi.fn()
const assignParked = vi.fn()
const unpark = vi.fn()
const dropFile = vi.fn()
const explainFile = vi.fn()
const deepTagStatus = vi.fn()
const push = vi.fn()

/** 捕获 deep-tag-progress 的订阅回调，测试里直接调用它模拟后端推事件 */
let deepCb: ((s: DeepTagStats) => void) | null = null

vi.mock('../../core/ipc', () => ({
  bootState,
  listParked,
  assignParked,
  unpark,
  dropFile,
  explainFile,
  deepTagStatus,
}))
vi.mock('../../core/events', () => ({
  onDeepTagProgress: vi.fn((cb: (s: DeepTagStats) => void) => {
    deepCb = cb
    // 与真实 events.ts 同形：返回 Promise<UnlistenFn>
    return Promise.resolve(() => {})
  }),
}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const Parked = (await import('./Parked.vue')).default

function file(over: Partial<FileRow> = {}): FileRow {
  return {
    id: 1,
    name: '安全检查通知.docx',
    ext: 'docx',
    kind: 'word',
    size: 24576,
    mtime: 1_700_000_000_000,
    status: 'parked',
    relPath: '安全检查通知.docx',
    absPath: 'C:/Users/kingb/Desktop/安全检查通知.docx',
    sourceRootId: 1,
    deferCount: 1,
    note: null,
    targetName: null,
    summary: '2026 年度安全检查通知，要求各部门 11 月底前自查上报。',
    tagLevel: 'deep',
    ...over,
  }
}

function item(over: Partial<ParkedItem> = {}, f: Partial<FileRow> = {}): ParkedItem {
  return {
    file: file(f),
    candidates: [
      {
        dirId: 7,
        relPath: '安全环保/检查报告',
        score: 0.72,
        reason: '与「安全环保/检查报告」下 6 份文件同类（都含「安全检查」）',
        uncertain: false,
      },
      { dirId: 9, relPath: '行政管理', score: 0.41, reason: '标签「通知」一致', uncertain: true },
    ],
    needsYou: false,
    ...over,
  }
}

async function mountWith(rows: ParkedItem[], deep: DeepTagStats | null = null) {
  listParked.mockResolvedValue(rows)
  deepTagStatus.mockResolvedValue(
    deep ?? {
      done: 0,
      failed: 0,
      remaining: 0,
      current: '',
      currentId: null,
      running: false,
      paused: false,
      enabled: true,
      note: null,
    },
  )
  bootState.mockResolvedValue({
    configured: true,
    archiveRoot: 'D:/文档归档',
    sourceRoots: [{ id: 1, path: 'C:/桌面', enabled: true }],
    counts: {
      queued: 0,
      labeled: 0,
      pendingConfirm: 0,
      confirmed: 0,
      migrated: 0,
      parked: rows.length,
      skipped: 0,
      missing: 0,
    },
    model: { reachable: true, model: 'qwen2.5:7b' },
    watchEnabled: true,
  })
  const w = mount(Parked)
  await flushPromises()
  return w
}

describe('待定区（P2 T4.3）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset()
    listParked.mockReset()
    assignParked.mockReset().mockResolvedValue(undefined)
    unpark.mockReset().mockResolvedValue(undefined)
    dropFile.mockReset().mockResolvedValue(undefined)
    explainFile.mockReset().mockResolvedValue({ queued: true })
    deepTagStatus.mockReset()
    deepCb = null
    push.mockReset()
  })

  it('几份就渲染几行，文件名/类型/大小都在', async () => {
    const w = await mountWith([
      item({}, { id: 1, name: 'a.docx' }),
      item({}, { id: 2, name: 'b.xlsx', kind: 'excel' }),
      item({}, { id: 3, name: 'c.pdf', kind: 'pdf' }),
    ])
    expect(w.findAll('.row')).toHaveLength(3)
    expect(w.text()).toContain('a.docx')
    expect(w.text()).toContain('c.pdf')
    expect(w.text()).toContain('PDF')
  })

  it('没有 summary 就写「未解读」，不做假占位', async () => {
    const w = await mountWith([
      item({}, { id: 1, summary: null, tagLevel: 'basic' }),
      item({}, { id: 2, summary: '读过的那份。' }),
    ])
    const sums = w.findAll('.sum')
    expect(sums[0].text()).toBe('未解读')
    expect(sums[0].classes()).toContain('none')
    expect(sums[1].text()).toBe('读过的那份。')
  })

  it('点主推目录 = 直接指定（origin=user_picked），行随即消失', async () => {
    const w = await mountWith([item(), item({}, { id: 2, name: 'b.docx' })])
    const first = w.findAll('.row')[0]
    const primary = first.findAll('button').find((b) => b.text().includes('主推'))!
    expect(primary.text()).toContain('安全环保/检查报告')

    await primary.trigger('click')
    await flushPromises()

    expect(assignParked).toHaveBeenCalledWith(1, 7, 'user_picked')
    expect(w.findAll('.row')).toHaveLength(1)
  })

  it('捞回与移出各自调对命令（不是同一个动作）', async () => {
    const w = await mountWith([item(), item({}, { id: 2, name: 'b.docx' })])
    const rows = w.findAll('.row')
    await rows[0].findAll('button').find((b) => b.text() === '捞回')!.trigger('click')
    await flushPromises()
    expect(unpark).toHaveBeenCalledWith(1)
    expect(dropFile).not.toHaveBeenCalled()

    const left = w.findAll('.row')[0]
    await left.findAll('button').find((b) => b.text() === '移出')!.trigger('click')
    await flushPromises()
    expect(dropFile).toHaveBeenCalledWith(2)
    expect(w.findAll('.row')).toHaveLength(0)
  })

  it('空态是中性的：不许出现「N 份未处理」这类催办语', async () => {
    const w = await mountWith([])
    const text = w.text()
    expect(text).toContain('这里现在是空的')
    expect(text).not.toContain('待处理')
    expect(text).not.toContain('未处理')
    expect(text).not.toContain('请尽快')
  })

  it('没有候选时说清「得你自己建一个」，而不是给一个空按钮区', async () => {
    const w = await mountWith([item({ candidates: [], needsYou: true })])
    expect(w.text()).toContain('没有足够像的目录')
    expect(w.findAll('.acts button').map((b) => b.text())).toEqual(['捞回', '移出'])
  })
})

// ------------------------------------------------ 标签一行（P3 T1.2）

describe('待定区标签行：与卡片同一套语义', () => {
  it('有标签就显示一行 chips；topic 带 .topic 类；超过 6 个折叠成 +N', async () => {
    const tags = [
      { id: 1, name: 'word', kind: 'type' },
      { id: 2, name: '2026', kind: 'time' },
      ...[3, 4, 5, 6, 7, 8].map((i) => ({ id: i, name: `话题${i}`, kind: 'topic' })),
    ] as FileRow['tags']
    const w = await mountWith([item({}, { tags })])
    const line = w.find('.tags')
    expect(line.exists()).toBe(true)
    const chips = line.findAll('.tchip')
    // 6 颗 + 1 个 +N
    expect(chips.length).toBe(7)
    expect(chips[2].classes()).toContain('topic')
    expect(w.find('.tchip.more').text()).toBe('+2')
    // title 里是全量名字
    expect(line.attributes('title')).toContain('话题8')
  })

  it('没标签的文件不渲染标签行（不留空行）', async () => {
    const w = await mountWith([item({}, { tags: [] })])
    expect(w.find('.tags').exists()).toBe(false)
  })
})

// ------------------------------------------------ 让 AI 说明（P3 T4.5）

describe('待定区「让 AI 说明」三态', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    bootState.mockReset()
    listParked.mockReset()
    assignParked.mockReset().mockResolvedValue(undefined)
    unpark.mockReset().mockResolvedValue(undefined)
    dropFile.mockReset().mockResolvedValue(undefined)
    explainFile.mockReset().mockResolvedValue({ queued: true })
    deepTagStatus.mockReset()
    deepCb = null
    push.mockReset()
  })

  function deepStats(over: Partial<DeepTagStats> = {}): DeepTagStats {
    return {
      done: 0,
      failed: 0,
      remaining: 0,
      current: '',
      currentId: null,
      running: false,
      paused: false,
      enabled: true,
      note: null,
      ...over,
    }
  }

  it('未解读的文件有「让 AI 看看这是什么」按钮；点了变「正在读…」并锁住', async () => {
    const w = await mountWith([
      item({}, { id: 1, summary: null, tagLevel: 'basic' }),
    ])
    const btn = w.findAll('.acts button').find((b) => b.text().includes('让 AI'))!
    expect(btn.exists()).toBe(true)

    await btn.trigger('click')
    await flushPromises()
    expect(explainFile).toHaveBeenCalledWith(1)

    const reading = w.findAll('.acts button').find((b) => b.text() === '正在读…')!
    expect(reading.exists()).toBe(true)
    expect(reading.attributes('disabled')).toBeDefined()
  })

  it('已经有解读的文件不给按钮（原地显示 summary 就够了）', async () => {
    const w = await mountWith([item()])
    const texts = w.findAll('.acts button').map((b) => b.text())
    expect(texts).not.toContain('让 AI 看看这是什么')
    expect(texts).toEqual(expect.arrayContaining(['捞回', '移出']))
  })

  it('深度解读关着时：说「深度解读已关闭」并给「去开启」，不静默失败', async () => {
    const w = await mountWith(
      [item({}, { id: 1, summary: null, tagLevel: 'basic' })],
      deepStats({ enabled: false }),
    )
    expect(w.text()).toContain('深度解读已关闭')
    const go = w.findAll('button').find((b) => b.text() === '去开启')!
    expect(go.exists()).toBe(true)
    await go.trigger('click')
    expect(push).toHaveBeenCalledWith('/settings')
    expect(explainFile).not.toHaveBeenCalled()
  })

  it('读到后原地更新：事件推进 done → 静默刷新 → summary 显示、按钮消失', async () => {
    const w = await mountWith([
      item({}, { id: 1, summary: null, tagLevel: 'basic' }),
    ])
    await w.findAll('.acts button').find((b) => b.text().includes('让 AI'))!.trigger('click')
    await flushPromises()

    // 后端读完：done 前进、currentId 清空；listParked 这次返回带 summary 的行
    listParked.mockResolvedValue([
      item({}, { id: 1, summary: '这是一份安全检查通知。', tagLevel: 'deep' }),
    ])
    deepCb!(
      deepStats({ done: 1, currentId: null, running: false, remaining: 0 }),
    )
    await flushPromises()

    expect(w.find('.sum').text()).toBe('这是一份安全检查通知。')
    const texts = w.findAll('.acts button').map((b) => b.text())
    expect(texts).not.toContain('让 AI 看看这是什么')
    expect(texts).not.toContain('正在读…')
    expect(texts).toEqual(expect.arrayContaining(['捞回', '移出']))
  })
})
