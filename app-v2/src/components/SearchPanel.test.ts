import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import type { FileRow } from '../core/types'

const searchFiles = vi.fn()
const openPath = vi.fn()
const push = vi.fn()
vi.mock('../core/ipc', () => ({ searchFiles, openPath }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const SearchPanel = (await import('./SearchPanel.vue')).default

function row(over: Partial<FileRow> = {}): FileRow {
  return {
    id: 1, name: '安全检查通知.docx', ext: 'docx', kind: 'word', size: 100,
    mtime: 1_700_000_000_000, status: 'migrated', relPath: '安全检查/通知.docx',
    absPath: 'C:/src/通知.docx', sourceRootId: 1, deferCount: 0, note: null,
    archivedPath: 'D:/文档归档/安全检查/通知.docx', archiveDirId: 7, tags: [],
    ...over,
  }
}

// 从 body 派发才能模拟「事件从目标元素往上冒、面板在 capture 阶段拦截」的真实次序
// （直接派发到 window 上 capture/bubble 不分层，测不出拦截语义）
const type = (w: ReturnType<typeof mount>, key: string, opts: Record<string, unknown> = {}) =>
  document.body.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts }),
  )

beforeEach(() => {
  searchFiles.mockReset().mockResolvedValue({ items: [row()], total: 1 })
  openPath.mockReset().mockResolvedValue(undefined)
  push.mockReset()
})

describe('Ctrl+K 命令面板（T3.3）', () => {
  it('Ctrl+K 开、Esc 关；关闭后查不到状态', async () => {
    const w = mount(SearchPanel, { global: { stubs: { teleport: true } } })
    type(w, 'k', { ctrlKey: true })
    await flushPromises()
    expect(w.find('.panel').exists()).toBe(true)
    type(w, 'Escape')
    await flushPromises()
    expect(w.find('.panel').exists()).toBe(false)
  })

  it('输入防抖后调 searchFiles({q, scope:"all", limit:20})', async () => {
    vi.useFakeTimers()
    const w = mount(SearchPanel, { global: { stubs: { teleport: true } } })
    type(w, 'k', { ctrlKey: true })
    await flushPromises()
    const input = w.find('.q')
    ;(input.element as HTMLInputElement).value = '报表'
    await input.trigger('input')
    expect(searchFiles).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(120)
    await flushPromises()
    expect(searchFiles).toHaveBeenCalledWith({ q: '报表', scope: 'all', limit: 20 })
    vi.useRealTimers()
  })

  it('面板打开时敲 p 不漏给其他监听者（整理台键位的红灯）', async () => {
    const leaked: string[] = []
    const listener = (e: KeyboardEvent) => leaked.push(e.key)
    window.addEventListener('keydown', listener) // 冒泡阶段 —— 相当于整理台的监听
    const w = mount(SearchPanel, { global: { stubs: { teleport: true } } })
    type(w, 'k', { ctrlKey: true })
    await flushPromises()
    type(w, 'p')
    await flushPromises()
    expect(leaked).not.toContain('p') /* 面板打开时普通按键必须被吞掉 */
    // 关闭后放行
    type(w, 'Escape')
    type(w, 'p')
    await flushPromises()
    expect(leaked).toContain('p')
    window.removeEventListener('keydown', listener)
  })

  it('↑↓ 移动光标，回车打开 livePath', async () => {
    searchFiles.mockResolvedValue({ items: [row(), row({ id: 2, name: 'b.pdf', kind: 'pdf', archivedPath: null })], total: 2 })
    const w = mount(SearchPanel, { global: { stubs: { teleport: true } } })
    type(w, 'k', { ctrlKey: true })
    await flushPromises()
    await w.find('.q').setValue('x')
    await w.find('.q').trigger('input')
    await vi.waitFor(() => expect(w.findAll('.item').length).toBe(2))
    type(w, 'ArrowDown')
    await flushPromises()
    expect(w.findAll('.item')[1].classes()).toContain('on')
    type(w, 'Enter')
    await flushPromises()
    expect(openPath).toHaveBeenCalledWith('C:/src/通知.docx') // 光标在第 2 个 → 源路径（未归档）
    expect(w.find('.panel').exists()).toBe(false) /* 打开后面板关闭 */
  })

  it('每行显示 文件名 · 目录 · 类型', async () => {
    const w = mount(SearchPanel, { global: { stubs: { teleport: true } } })
    type(w, 'k', { ctrlKey: true })
    await flushPromises()
    await w.find('.q').setValue('x')
    await w.find('.q').trigger('input')
    await vi.waitFor(() => expect(w.findAll('.item').length).toBe(1))
    const item = w.find('.item')
    expect(item.find('.iname').text()).toBe('安全检查通知.docx')
    expect(item.find('.idir').text()).toBe('安全检查')
    expect(item.find('.ikind').text()).toBe('文档')
  })

  it('无结果给「没有匹配的文件」+「去高级筛选」，点击带 q 跳转', async () => {
    searchFiles.mockResolvedValue({ items: [], total: 0 })
    const w = mount(SearchPanel, { global: { stubs: { teleport: true } } })
    type(w, 'k', { ctrlKey: true })
    await flushPromises()
    await w.find('.q').setValue('不存在的词')
    await w.find('.q').trigger('input')
    await vi.waitFor(() => expect(w.text()).toContain('没有匹配的文件'))
    await w.find('.link').trigger('click')
    expect(push).toHaveBeenCalledWith({ path: '/search', query: { q: '不存在的词' } })
  })
})
