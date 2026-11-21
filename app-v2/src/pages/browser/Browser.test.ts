import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import type { DirNode, FileRow } from '../../core/types'

const browseTree = vi.fn()
const browseDirFiles = vi.fn()
const poolRows = vi.fn()
const listParked = vi.fn()
const openPath = vi.fn()
const revealPath = vi.fn()
const searchFilesSpy = vi.fn()
const push = vi.fn()

vi.mock('../../core/ipc', () => ({
  browseTree,
  browseDirFiles,
  poolRows,
  listParked,
  openPath,
  revealPath,
  searchFiles: searchFilesSpy,
}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const Browser = (await import('./Browser.vue')).default

function file(over: Partial<FileRow> = {}): FileRow {
  return {
    id: 1,
    name: '安全检查通知.docx',
    ext: 'docx',
    kind: 'word',
    size: 24576,
    mtime: 1_700_000_000_000,
    status: 'migrated',
    relPath: '安全检查/通知.docx',
    absPath: 'C:/Users/kingb/Desktop/安全检查通知.docx',
    sourceRootId: 1,
    deferCount: 0,
    note: null,
    summary: '2026 年度安全检查通知。',
    tagLevel: 'deep',
    archivedPath: 'D:/文档归档/安全检查/安全检查通知.docx',
    archiveDirId: 7,
    tags: [
      { id: 1, name: '安全检查', kind: 'topic' },
      { id: 2, name: '重点', kind: 'user' },
    ],
    ...over,
  }
}

function node(over: Partial<DirNode> = {}): DirNode {
  return {
    id: 7,
    name: '安全检查',
    relPath: '安全检查',
    depth: 1,
    origin: 'user',
    fileCount: 2,
    children: [],
    ...over,
  }
}

beforeEach(() => {
  browseTree.mockReset().mockResolvedValue([node()])
  browseDirFiles.mockReset().mockResolvedValue([file()])
  poolRows.mockReset().mockResolvedValue([file({ id: 2, status: 'queued', archivedPath: null, archiveDirId: null })])
  listParked.mockReset().mockResolvedValue([file({ id: 3, status: 'parked' })])
  openPath.mockReset().mockResolvedValue(undefined)
  revealPath.mockReset().mockResolvedValue(undefined)
  searchFilesSpy.mockReset().mockResolvedValue({ items: [], total: 0 })
  push.mockReset()
})

async function mountBrowser() {
  const w = mount(Browser)
  await flushPromises()
  return w
}

describe('浏览页 · 目录树与列表（T2.4）', () => {
  it('树渲染：归档目录带直属份数', async () => {
    const w = await mountBrowser()
    expect(browseTree).toHaveBeenCalled()
    expect(w.text()).toContain('安全检查')
    expect(w.text()).toContain('2')
  })

  it('选中目录切换列表：browseDirFiles 带目录 id', async () => {
    const w = await mountBrowser()
    browseDirFiles.mockClear()
    await w.findAll('.vrow').find((r) => r.text().includes('未归类'))!.trigger('click')
    await flushPromises()
    expect(browseDirFiles).toHaveBeenCalledWith(null, false)
  })

  it('列表六格：名称 / 类型 / 大小 / 时间 / 标签 / 状态', async () => {
    const w = await mountBrowser()
    const row = w.find('.list .row')
    const cells = row.findAll('.cell')
    expect(cells.length).toBe(6)
    expect(cells[0].text()).toContain('安全检查通知.docx')
    expect(cells[1].text()).toBe('文档')
    expect(cells[4].text()).toContain('安全检查')
    expect(cells[5].text()).not.toBe('')
  })

  it('双击打开走 livePath（已归档用归档路径，不是已进隔离区的源路径）', async () => {
    const w = await mountBrowser()
    await w.find('.list .row').trigger('dblclick')
    expect(openPath).toHaveBeenCalledWith('D:/文档归档/安全检查/安全检查通知.docx')
  })

  it('空态：「这个目录还没有文件」+「去整理台看看」，点击去整理台', async () => {
    browseDirFiles.mockResolvedValue([])
    const w = await mountBrowser()
    expect(w.text()).toContain('这个目录还没有文件')
    await w.find('.link').trigger('click')
    expect(push).toHaveBeenCalledWith('/')
  })

  it('右键菜单只有三项 —— 锁死「不出现重新归类」', async () => {
    const w = await mountBrowser()
    await w.find('.list .row').trigger('contextmenu', { clientX: 10, clientY: 10 })
    await flushPromises()
    const items = w.findAll('.menu .mi')
    expect(items.map((b) => b.text())).toEqual(['打开', '在资源管理器中定位', '复制路径'])
    expect(w.text()).not.toContain('重新归类')
  })

  it('「复制路径」写的是 livePath', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const w = await mountBrowser()
    await w.find('.list .row').trigger('contextmenu', { clientX: 10, clientY: 10 })
    await w.findAll('.menu .mi').find((b) => b.text() === '复制路径')!.trigger('click')
    expect(writeText).toHaveBeenCalledWith('D:/文档归档/安全检查/安全检查通知.docx')
  })
})

describe('浏览页 · 虚拟视图与详情条（T2.5）', () => {
  it('两个虚拟节点带正确计数', async () => {
    const w = await mountBrowser()
    const texts = w.findAll('.vrow').map((r) => r.text())
    expect(texts.some((t) => t.includes('待整理') && t.includes('1'))).toBe(true)
    expect(texts.some((t) => t.includes('待定区') && t.includes('1'))).toBe(true)
  })

  it('点「待整理」列表来自 poolRows；点「待定区」来自 listParked', async () => {
    poolRows.mockResolvedValue([file({ id: 2, status: 'queued', name: '池子里的一份.docx', archivedPath: null, archiveDirId: null })])
    listParked.mockResolvedValue([
      { file: file({ id: 3, status: 'parked', name: '待定区的一份.docx' }), candidates: [], needsYou: false },
    ])
    const w = await mountBrowser()
    await w.findAll('.vrow').find((r) => r.text().includes('待整理'))!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('池子里的一份.docx')
    await w.findAll('.vrow').find((r) => r.text().includes('待定区'))!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('待定区的一份.docx')
  })

  it('详情条显示 summary、标签与 livePath；没 summary 显示「未解读」', async () => {
    const w = await mountBrowser()
    await w.find('.list .row').trigger('click')
    await flushPromises()
    const d = w.find('.detail')
    expect(d.exists()).toBe(true)
    expect(d.text()).toContain('2026 年度安全检查通知。')
    expect(d.text()).toContain('重点')
    expect(d.find('.dpath').text()).toBe('D:/文档归档/安全检查/安全检查通知.docx')
  })

  it('没 summary 的行显示「未解读」，不显示标签行', async () => {
    browseDirFiles.mockResolvedValue([file({ id: 9, summary: null, tags: [] })])
    const w = await mountBrowser()
    await w.find('.list .row').trigger('click')
    await flushPromises()
    const d = w.find('.detail')
    expect(d.exists()).toBe(true)
    expect(d.find('.dsummary').text()).toBe('未解读')
    expect(d.find('.dtags').exists()).toBe(false)
  })
})

describe('目录内筛选（T3.5）', () => {
  it('输入后请求带 dirId（与树选择联动），150ms 防抖后走 searchFiles', async () => {
    vi.useFakeTimers()
    const w = await mountBrowser()
    await w.find('.tree .row').trigger('click') // 先选中一个真实目录（id=7）
    await flushPromises()
    searchFilesSpy.mockClear()
    await w.find('.filter').setValue('报表')
    expect(searchFilesSpy).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(200)
    expect(searchFilesSpy).toHaveBeenCalledWith(
      expect.objectContaining({ q: '报表', dirId: 7, scope: 'archive' }),
    )
    vi.useRealTimers()
  })

  it('清空后请求不带 q，列表恢复全量', async () => {
    vi.useFakeTimers()
    const w = await mountBrowser()
    await w.find('.filter').setValue('报表')
    await vi.advanceTimersByTimeAsync(200)
    expect(searchFilesSpy).toHaveBeenCalledWith(expect.objectContaining({ q: '报表' }))
    browseDirFiles.mockClear()
    await w.find('.clearbtn').trigger('click')
    await flushPromises()
    // 清空后恢复走全量列表（browse_dir），不再带检索条件
    expect(browseDirFiles).toHaveBeenCalled()
    vi.useRealTimers()
  })
})
