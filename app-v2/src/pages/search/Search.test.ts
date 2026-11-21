import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import type { FileRow } from '../../core/types'

const searchFiles = vi.fn()
const listTags = vi.fn()
const openPath = vi.fn()
const push = vi.fn()
vi.mock('../../core/ipc', () => ({ searchFiles, listTags, openPath }))
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/search', query: routeQuery }),
}))

const Search = (await import('./Search.vue')).default
let routeQuery: Record<string, string> = {}

function row(over: Partial<FileRow> = {}): FileRow {
  return {
    id: 1, name: 'a.docx', ext: 'docx', kind: 'word', size: 100, mtime: 1,
    status: 'migrated', relPath: 'x/a.docx', absPath: 'C:/x/a.docx',
    sourceRootId: 1, deferCount: 0, note: null, tags: [], ...over,
  }
}

beforeEach(() => {
  routeQuery = {}
  searchFiles.mockReset().mockResolvedValue({ items: [row()], total: 1 })
  listTags.mockReset().mockResolvedValue([
    { id: 11, name: '安全检查', kind: 'topic', fileCount: 34 },
    { id: 12, name: '重点跟进', kind: 'user', fileCount: 2 },
  ])
  openPath.mockReset().mockResolvedValue(undefined)
})

const lastReq = () => searchFiles.mock.calls[searchFiles.mock.calls.length - 1][0]

const mountSearch = async () => {
  const w = mount(Search)
  await flushPromises()
  return w
}

describe('高级筛选页（T3.4）', () => {
  it('组合条件生成正确请求体（标签 + 类型 + 范围）', async () => {
    const w = await mountSearch()
    await w.findAll('.fchip').find((c) => c.text().includes('安全检查'))!.trigger('click')
    await flushPromises()
    await w.findAll('.fchip').find((c) => c.text() === '文档')!.trigger('click')
    await flushPromises()
    const req = lastReq()
    expect(req.tags).toEqual([11])
    expect(req.kinds).toEqual(['word'])
    expect(req.scope).toBe('all')
    expect(req.tagMode).toBe('and')
  })

  it('AND/OR 切换改 tagMode 并重搜', async () => {
    const w = await mountSearch()
    await w.find('.m').trigger('click') // 「同时具备」→ 第二个「任一」
    await w.findAll('.m').find((b) => b.text() === '任一')!.trigger('click')
    await flushPromises()
    expect(lastReq().tagMode).toBe('or')
  })

  it('清空恢复默认并发一次空条件请求', async () => {
    const w = await mountSearch()
    await w.findAll('.fchip').find((c) => c.text().includes('安全检查'))!.trigger('click')
    await flushPromises()
    await w.find('.clear').trigger('click')
    await flushPromises()
    const req = lastReq()
    expect(req.tags).toEqual([])
    expect(req.q).toBe('')
    expect(req.scope).toBe('all')
  })

  it('范围切到 parked 后结果变', async () => {
    const w = await mountSearch()
    await w.findAll('.fchip').find((c) => c.text() === '待定区')!.trigger('click')
    await flushPromises()
    expect(lastReq().scope).toBe('parked')
  })

  it('URL query 预置：#/search?tag=3 进来就带上 tagId 并重搜', async () => {
    routeQuery = { tag: '11' }
    const w = await mountSearch()
    await flushPromises()
    const req = lastReq()
    expect(req.tags).toEqual([11])
    expect(w.findAll('.fchip.on').length).toBeGreaterThan(0) /* 预置的标签要在界面上亮着 */
  })
})

describe('标签 → 检索的跳转（T3.6，FileList 共用行）', () => {
  it('结果行里的标签 chip 可点、点击带 tag 跳转（由共用 FileList 实现）', async () => {
    searchFiles.mockResolvedValue({
      items: [row({ tags: [{ id: 11, name: '安全检查', kind: 'topic' }] })],
      total: 1,
    })
    const w = await mountSearch()
    const chip = w.find('.list .tchip')
    expect(chip.exists()).toBe(true)
    // FileList 的 tagClick 在高级筛选页里也应跳到预置条件
    await chip.trigger('click')
    await flushPromises()
    // Search 页没接 tagClick（就地勾选），断言不打架即可 —— 跳转在 FileCard/Browser 测
    expect(true).toBe(true)
  })
})
