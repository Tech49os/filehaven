import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import type { DirNode, FileRow } from '../../core/types'

const browseTree = vi.fn()
const browseDirFiles = vi.fn()
const poolRows = vi.fn()
const listParked = vi.fn()
const openPath = vi.fn()
const revealPath = vi.fn()
const recategorizeFile = vi.fn()
const searchFilesSpy = vi.fn()
const push = vi.fn()

vi.mock('../../core/ipc', () => ({
  browseTree,
  browseDirFiles,
  poolRows,
  listParked,
  openPath,
  revealPath,
  recategorizeFile,
  searchFiles: searchFilesSpy,
}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const Browser = (await import('./Browser.vue')).default

// 测试环境当宽窗：happy-dom 自带 matchMedia 且默认窗宽 1024，
// 会让 useNarrow 判成窄窗、把左树收进抽屉，树的测试就全空了
beforeEach(() => {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (q: string) => ({
      matches: false,
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  })
})

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
  browseTree.mockReset().mockResolvedValue([node(), node({ id: 8, name: '整改台账', relPath: '整改台账' })])
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

  it('已归档文件右键菜单四项，含「重新归类…」；未归档文件没有这一项', async () => {
    const w = await mountBrowser()
    // 列表行是 migrated
    await w.find('.list .row').trigger('contextmenu', { clientX: 10, clientY: 10 })
    await flushPromises()
    const items = w.findAll('.menu .mi').map((b) => b.text())
    expect(items).toEqual([
      '打开',
      '在资源管理器中定位',
      '复制路径',
      '重新归类…',
    ])

    // 池内文件（queued）不许出现重新归类 —— P3 的锁改成条件断言
    await w.findAll('.vrow').find((r) => r.text().includes('待整理'))!.trigger('click')
    await flushPromises()
    await w.find('.list .row').trigger('contextmenu', { clientX: 10, clientY: 10 })
    await flushPromises()
    const poolItems = w.findAll('.menu .mi').map((b) => b.text())
    expect(poolItems).toEqual(['打开', '在资源管理器中定位', '复制路径'])
    expect(poolItems).not.toContain('重新归类…')
  })

  it('重新归类：点开后选目标目录 → 调后端 → 提示 + 刷新；当前目录不在候选里', async () => {
    recategorizeFile.mockResolvedValue({ newPath: 'D:/文档归档/整改台账/安全检查通知.docx', renamed: false })
    const w = await mountBrowser()
    await w.find('.list .row').trigger('contextmenu', { clientX: 10, clientY: 10 })
    await flushPromises()
    await w.findAll('.menu .mi').find((b) => b.text() === '重新归类…')!.trigger('click')
    await flushPromises()

    // 候选不含当前目录（archiveDirId=7），只剩「整改台账」
    const targets = w.findAll('.menu .mi').map((b) => b.text())
    expect(targets).toContain('挪到哪个目录？')
    expect(targets).toContain('整改台账')
    expect(targets).not.toContain('安全检查')

    await w.findAll('.menu .mi').find((b) => b.text() === '整改台账')!.trigger('click')
    await flushPromises()
    expect(recategorizeFile).toHaveBeenCalledWith(1, 8)
    expect(w.text()).toContain('已挪到「整改台账」')
  })

  it('重新归类失败时把后端原因亮出来，不静默', async () => {
    recategorizeFile.mockRejectedValue({ code: 'MIGRATE_FAILED', message: '归档文件已不在磁盘上' })
    const w = await mountBrowser()
    await w.find('.list .row').trigger('contextmenu', { clientX: 10, clientY: 10 })
    await w.findAll('.menu .mi').find((b) => b.text() === '重新归类…')!.trigger('click')
    await w.findAll('.menu .mi').find((b) => b.text() === '整改台账')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('归档文件已不在磁盘上')
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

describe('目录树默认折叠（BUG-002）', () => {
  // 3 层样例树：1 → 2 → 3 → 4，第 3 层有子节点应被默认折叠
  let deep: DirNode[]

  beforeEach(() => {
    deep = [
      node({
        id: 1, name: '一层', relPath: '一层', depth: 1,
        children: [
          node({
            id: 2, name: '二层', relPath: '一层/二层', depth: 2,
            children: [
              node({
                id: 3, name: '三层', relPath: '一层/二层/三层', depth: 3,
                children: [node({ id: 4, name: '四层', relPath: '一层/二层/三层/四层', depth: 4 })],
              }),
            ],
          }),
        ],
      }),
    ]
    browseTree.mockReset().mockResolvedValue(deep)
  })

  it('加载树后只展开到第 2 层：aria-expanded=true 只属于 depth 1、2', async () => {
    const w = await mountBrowser()
    const expanded = w.findAll('[role="treeitem"][aria-expanded="true"]').map((n) => n.text())
    expect(expanded.some((t) => t.includes('一层'))).toBe(true)
    expect(expanded.some((t) => t.includes('二层'))).toBe(true)
    expect(expanded.some((t) => t.includes('三层'))).toBe(false)
    // 第 4 层整行不渲染
    expect(w.text()).not.toContain('四层')
  })

  it('点第 3 层 caret 展开其子树，再点收起', async () => {
    const w = await mountBrowser()
    let caretOf3 = w.findAll('.caret').find((b) => b.attributes('aria-label') === '展开')
    expect(caretOf3, '第 3 层应处于折叠态').toBeTruthy()

    await caretOf3!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('四层')

    // 用所在行锚定第 3 层的 caret，再点一次恢复折叠
    const row3 = w.findAll('.row').find((r) => r.text().includes('三层'))!
    await row3.find('.caret').trigger('click')
    await flushPromises()
    expect(w.text()).not.toContain('四层')
    caretOf3 = w.findAll('.caret').find(
      (b) => b.attributes('aria-label') === '展开',
    )
    expect(caretOf3, '收起后第 3 层重新处于折叠态').toBeTruthy()
  })

  it('树加载与默认折叠同批落位：列表 IPC 未返回时深层已折叠，不闪全展开', async () => {
    let releaseFiles!: (rows: FileRow[]) => void
    browseDirFiles.mockReturnValueOnce(new Promise((resolve) => { releaseFiles = resolve }))

    const w = mount(Browser)
    await flushPromises() // browseTree 已回、browseDirFiles 仍挂起

    // 此刻树已渲染：第 3 层不能被画出（否则会先闪一帧全展开）
    expect(w.text()).toContain('三层')
    expect(w.text()).not.toContain('四层')

    releaseFiles([file()])
    await flushPromises()
  })

  it('手动展开深层后，点「仅看前两层」恢复默认折叠', async () => {
    const w = await mountBrowser()
    await w.findAll('.caret').find((b) => b.attributes('aria-label') === '展开')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('四层')

    const resetBtn = w.findAll('button').find((b) => b.text().includes('仅看前两层'))
    expect(resetBtn, '树头应有「仅看前两层」按钮').toBeTruthy()
    await resetBtn!.trigger('click')
    await flushPromises()
    expect(w.text()).not.toContain('四层')
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
