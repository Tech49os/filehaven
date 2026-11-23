import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import type { BatchItem, FilePreview } from '../core/types'
import fileCardSource from './FileCard.vue?raw'

/** 从组件源码里取一段 CSS 规则体；happy-dom 没有布局引擎，用 CSS 不变量守住几何规则 */
function cssRule(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const m = css.match(new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`, 'm'))
  return m ? m[1] : ''
}

// Tauri 的 asset 协议在测试里不存在，把它换成一个可断言的假 URL
vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (p: string) => `asset://localhost/${p}`,
}))

const FileCard = (await import('./FileCard.vue')).default

function item(over: Partial<BatchItem['file']> = {}): BatchItem {
  return {
    file: {
      id: 1,
      name: '合同.docx',
      ext: 'docx',
      kind: 'word',
      size: 20480,
      mtime: Date.now() - 3600_000,
      status: 'pending_confirm',
      // 相对路径与绝对路径故意不同：卡片必须显示绝对路径，否则用户不知道文件在哪
      relPath: '桌面/合同.docx',
      absPath: 'C:/Users/kingb/Desktop/合同.docx',
      sourceRootId: 1,
      deferCount: 0,
      note: null,
      ...over,
    },
    candidates: [
      { dirId: 7, relPath: '合同', score: 0.8, reason: '文件名含「合同」', uncertain: false },
    ],
    needsYou: false,
  }
}

const textPreview = (over: Partial<FilePreview> = {}): FilePreview => ({
  text: '甲方：某某公司　乙方：某某公司　合同金额：十二万元',
  truncated: false,
  reason: '',
  kind: 'text',
  ...over,
})

function mountCard(preview: FilePreview | null = null) {
  return mount(FileCard, {
    props: {
      item: item(),
      dirCount: 2,
      position: { index: 0, total: 3 },
      preview,
    },
  })
}

/* ---- BUG-001：happy-dom 无真实布局，mock 高度描述符驱动「是否溢出」，用完即恢复 ---- */
let mockScrollHeight: number | null = null
let mockClientHeight: number | null = null
let savedScroll: PropertyDescriptor | undefined
let savedClient: PropertyDescriptor | undefined

function installLayoutMock() {
  savedScroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollHeight')
  savedClient = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight')
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', {
    configurable: true,
    get() {
      return mockScrollHeight ?? savedScroll?.get?.call(this) ?? 0
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get() {
      return mockClientHeight ?? savedClient?.get?.call(this) ?? 0
    },
  })
}

function restoreLayoutMock() {
  if (savedScroll) Object.defineProperty(HTMLElement.prototype, 'scrollHeight', savedScroll)
  else delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollHeight
  if (savedClient) Object.defineProperty(HTMLElement.prototype, 'clientHeight', savedClient)
  else delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientHeight
  mockScrollHeight = null
  mockClientHeight = null
  savedScroll = undefined
  savedClient = undefined
}

function setLayout(scrollHeight: number, clientHeight: number) {
  mockScrollHeight = scrollHeight
  mockClientHeight = clientHeight
}

/** onMounted/watch 里都有 nextTick 测量，连刷两拍确保 DOM 更新到位 */
const flush = async () => {
  await nextTick()
  await nextTick()
}

describe('整理台卡片：路径 / 内容 / 打开（2026-11-20 反馈）', () => {
  beforeEach(() => {
    // 模拟应用内环境：有 Tauri 上下文才走 asset 协议
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
  })

  it('显示完整路径，不是只有文件名', () => {
    const w = mountCard()
    expect(w.text()).toContain('C:/Users/kingb/Desktop/合同.docx')
  })

  it('「打开」与「在文件夹中显示」各自发事件，由页面调系统', () => {
    const w = mountCard()
    const open = w.findAll('button').find((b) => b.text().includes('打开'))
    const reveal = w.findAll('button').find((b) => b.text().includes('定位'))
    expect(open).toBeTruthy()
    expect(reveal).toBeTruthy()
    void open!.trigger('click')
    void reveal!.trigger('click')
    expect(w.emitted('open')).toHaveLength(1)
    expect(w.emitted('reveal')).toHaveLength(1)
  })

  it('有正文时直接显示摘要（是否给展开由实测溢出决定，不看 truncated）', async () => {
    installLayoutMock()
    try {
      setLayout(120, 40) // 内容高于可见高度 → 真溢出
      const w = mountCard(textPreview({ truncated: false }))
      await flush()
      expect(w.text()).toContain('合同金额：十二万元')
      expect(w.find('.pv-toggle').exists()).toBe(true)
    } finally {
      restoreLayoutMock()
    }
  })

  it('读不出正文时给一句人话，不留空白框', () => {
    const w = mountCard({
      text: '',
      truncated: false,
      reason: 'PDF 这一版不抽正文，直接打开看最快',
      kind: 'none',
    })
    expect(w.text()).toContain('PDF 这一版不抽正文')
    expect(w.find('.pv-text').exists()).toBe(false)
  })

  it('图片走缩略图（asset 协议），不是文字摘要', () => {
    const w = mount(FileCard, {
      props: {
        item: item({
          name: 'IMG_2043.jpg',
          ext: 'jpg',
          kind: 'image',
          absPath: 'C:/Users/kingb/Pictures/IMG_2043.jpg',
        }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: { text: '', truncated: false, reason: '', kind: 'image' },
      },
    })
    const img = w.find('.pv-thumb img')
    expect(img.exists()).toBe(true)
    expect(img.attributes('src')).toContain('asset://localhost/')
    expect(img.attributes('src')).toContain('IMG_2043.jpg')
  })

  it('还没有预览结果时不占位（不闪空框）', () => {
    const w = mountCard(null)
    expect(w.find('.preview').exists()).toBe(false)
  })

  // ------------------------------------------------ 归档前改名（2026-11-20 反馈）

  it('点「改名」只出主干，扩展名固定显示，回车发出 rename', async () => {
    const w = mountCard()
    await w.find('.ren').trigger('click')
    const input = w.find('.rename-input')
    expect((input.element as HTMLInputElement).value).toBe('合同')
    expect(w.find('.rename-ext').text()).toBe('.docx')

    await input.setValue('采购合同-2026终版')
    await input.trigger('keydown', { key: 'Enter' })
    expect(w.emitted('rename')?.[0]).toEqual(['采购合同-2026终版'])
  })

  it('名字没动就关掉输入框，不发 IPC', async () => {
    const w = mountCard()
    await w.find('.ren').trigger('click')
    await w.find('.rename-input').trigger('keydown', { key: 'Escape' })
    expect(w.find('.rename-input').exists()).toBe(false)
    expect(w.emitted('rename')).toBeUndefined()
  })

  it('改过名之后卡片一直说清「归档时存为」，并给还原入口', async () => {
    const w = mount(FileCard, {
      props: {
        item: item({ targetName: '采购合同-2026终版.docx' }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    expect(w.find('.name').text()).toBe('合同.docx')
    expect(w.find('.newname').text()).toContain('采购合同-2026终版.docx')

    const reset = w.findAll('.newname .link').find((b) => b.text() === '用原名')!
    await reset.trigger('click')
    expect(w.emitted('rename')?.[0]).toEqual([''])
  })

  it('已确认的卡片不再给改名入口', () => {
    const w = mount(FileCard, {
      props: {
        item: item({ status: 'confirmed' }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
        ack: { fileId: 1, status: 'confirmed', confirmed: 1, targetRelPath: '合同', remainingInPool: 3 },
      },
    })
    expect(w.find('.ren').exists()).toBe(false)
  })
})

// ---------------------------------------------------------------- 深度解读（P2 T2.7）

describe('整理台卡片：summary 与标签层级', () => {
  it('有 summary 就显示，一行读完知道「这是什么」', () => {
    const w = mount(FileCard, {
      props: {
        item: item({ summary: '2026 年度配电箱巡检记录，配电班组。', tagLevel: 'deep' }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    expect(w.find('.summary').exists()).toBe(true)
    expect(w.find('.summary').text()).toContain('配电箱巡检记录')
  })

  it('没有 summary 就不占位（不留空行、不写「未解读」假占位）', () => {
    const w = mount(FileCard, {
      props: {
        item: item({ summary: null, tagLevel: 'basic' }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    expect(w.find('.summary').exists()).toBe(false)
  })

  it('tagLevel=deep 的层级标签是实心，basic 是空心', () => {
    const deep = mount(FileCard, {
      props: {
        item: item({ tagLevel: 'deep' }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    const lvl = deep.find('.taglvl')
    expect(lvl.exists()).toBe(true)
    expect(lvl.classes()).toContain('deep')
    expect(lvl.text()).toBe('已解读')

    const basic = mount(FileCard, {
      props: {
        item: item({ tagLevel: 'basic' }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    expect(basic.find('.taglvl').classes()).not.toContain('deep')
    expect(basic.find('.taglvl').text()).toBe('初步')
  })
})

// ------------------------------------------------ 长名字 / 长路径（P2 T5.2）

describe('长文件名与长路径：单行不换高、省略但 title 有全文', () => {
  beforeEach(() => {
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
  })

  const longName =
    '2026年度智能工厂平台安全生产大检查暨消防设施专项验收工作情况汇报（第三次修订版）最终定稿v3.docx'

  it('120 字文件名也不许换行：单行 ellipsis + title 全文', () => {
    const w = mount(FileCard, {
      props: {
        item: item({ name: longName }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    const h = w.find('.name')
    expect(h.text()).toBe(longName) // 文本不裁，靠 CSS 截
    expect(h.attributes('title')).toBe(longName)
  })

  it('长路径中间省略：左边留盘符，右边留哪一份，title 里是全文', () => {
    // 用正斜杠形式：Tauri 侧回传的 absPath 也是 `C:/Users/...` 这种
    const abs = 'C:/Users/kingb/Desktop/2026/管理/安全检查通知.docx'
    const w = mount(FileCard, {
      props: {
        item: item({ absPath: abs }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
      },
    })
    const p = w.find('.pval')
    expect(p.text()).toBe('C:/…/管理/安全检查通知.docx')
    expect(p.attributes('title')).toBe(abs)
  })
})

// ------------------------------------------------ 标签行（P3 T1.2）

describe('卡片标签行：看得见、可删、超 6 折叠', () => {
  beforeEach(() => {
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
  })

  const tag = (id: number, name: string, kind: 'type' | 'time' | 'topic' | 'keyword' | 'user') => ({
    id,
    name,
    kind,
  })

  it('0 标签：容器整个不渲染，不留空行', () => {
    const w = mount(FileCard, {
      props: { item: item({ tags: [] }), dirCount: 2, position: { index: 0, total: 1 }, preview: null },
    })
    expect(w.find('.tagsrow').exists()).toBe(false)
  })

  it('5 个全显，顺序与 kind 类名按后端给的来', () => {
    const tags = [
      tag(1, 'word', 'type'),
      tag(2, '2026', 'time'),
      tag(3, '安全检查', 'topic'),
      tag(4, '通知', 'keyword'),
      tag(5, '重点', 'user'),
    ]
    const w = mount(FileCard, {
      props: { item: item({ tags }), dirCount: 2, position: { index: 0, total: 1 }, preview: null },
    })
    const chips = w.findAll('.tagsrow .chip')
    expect(chips.length).toBe(5)
    expect(chips[2].classes()).toContain('topic')
    expect(chips[4].classes()).toContain('user')
  })

  it('7 个显 6 个 + 「+1」，+N 的 title 给全量', () => {
    const tags = [
      tag(1, 'word', 'type'),
      tag(2, '2026', 'time'),
      ...[3, 4, 5, 6].map((i) => tag(i, `话题${i}`, 'topic' as const)),
      tag(7, '排在最后的那个', 'user'),
    ]
    const w = mount(FileCard, {
      props: { item: item({ tags }), dirCount: 2, position: { index: 0, total: 1 }, preview: null },
    })
    const chips = w.findAll('.tagsrow .chip')
    expect(chips.length).toBe(7) // 6 颗 + 1 个 +N
    expect(w.find('.chip.more').text()).toBe('+1')
    expect(w.find('.chip.more').attributes('title')).toBe('排在最后的那个')
  })

  it('user 标签带 .user 类；type/time 不给删除按钮，topic 给', () => {
    const tags = [tag(1, 'word', 'type'), tag(2, '安全检查', 'topic'), tag(3, '重点', 'user')]
    const w = mount(FileCard, {
      props: { item: item({ tags }), dirCount: 2, position: { index: 0, total: 1 }, preview: null },
    })
    // type 不可删，topic/user 可删
    expect(w.findAll('.chip .x').length).toBe(2)
    // 点 topic 的 × 发 removeTag 事件
    w.findAll('.chip .x')[0].trigger('click')
    expect(w.emitted('removeTag')![0]).toEqual([2])
  })
})

describe('标签 → 检索的跳转（T3.6）', () => {
  beforeEach(() => {
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
  })

  it('Ctrl+点标签发 tagClick；普通点击不发（整理台不能误触）', async () => {
    const tags = [
      { id: 3, name: '安全检查', kind: 'topic' as const },
      { id: 5, name: '重点', kind: 'user' as const },
    ]
    const w = mount(FileCard, {
      props: { item: item({ tags }), dirCount: 2, position: { index: 0, total: 1 }, preview: null },
    })
    const chip = w.findAll('.tagsrow .chip')[0]
    await chip.trigger('click') // 普通点击
    expect(w.emitted('tagClick')).toBeUndefined()
    await chip.trigger('click', { ctrlKey: true })
    expect(w.emitted('tagClick')![0]).toEqual([3])
  })
})

// ------------------------------------- BUG-001：正文摘要「实际溢出」才给展开入口
describe('BUG-001：摘要溢出与展开', () => {
  beforeEach(() => {
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}
    installLayoutMock()
  })
  afterEach(() => {
    restoreLayoutMock()
  })

  it('溢出（scrollHeight>clientHeight）时显示展开按钮，即便 truncated=false', async () => {
    setLayout(200, 50)
    const w = mountCard(textPreview({ truncated: false }))
    await flush()
    expect(w.find('.pv-toggle').exists()).toBe(true)
    expect(w.find('.pv-toggle').text()).toContain('展开全文')
  })

  it('未溢出时不显示按钮，即便 truncated=true', async () => {
    setLayout(50, 50)
    const w = mountCard(textPreview({ truncated: true }))
    await flush()
    expect(w.find('.pv-toggle').exists()).toBe(false)
  })

  it('展开：class 含 full、文案变「收起」；再点恢复 4 行', async () => {
    setLayout(200, 50)
    const w = mountCard(textPreview({ truncated: false }))
    await flush()
    await w.find('.pv-toggle').trigger('click')
    expect(w.find('.pv-text').classes()).toContain('full')
    expect(w.find('.pv-toggle').text()).toContain('收起')
    await w.find('.pv-toggle').trigger('click')
    expect(w.find('.pv-text').classes()).not.toContain('full')
    expect(w.find('.pv-toggle').text()).toContain('展开全文')
  })

  it('换 file.id 后展开态复位为收起', async () => {
    setLayout(200, 50)
    const w = mountCard(textPreview({ truncated: false }))
    await flush()
    await w.find('.pv-toggle').trigger('click')
    expect(w.find('.pv-text').classes()).toContain('full')

    await w.setProps({ item: item({ id: 2 }) })
    await flush()
    expect(w.find('.pv-text').classes()).not.toContain('full')
    expect(w.find('.pv-toggle').text()).toContain('展开全文')
  })

  it('图片 / 读不出原因分支不渲染展开按钮', async () => {
    setLayout(200, 50)
    const none = mountCard({
      text: '',
      truncated: true,
      reason: 'PDF 这一版不抽正文，直接打开看最快',
      kind: 'none',
    })
    await flush()
    expect(none.find('.pv-toggle').exists()).toBe(false)
  })

  it('展开态下 resize 重算不清除溢出标记，仍能收起', async () => {
    setLayout(200, 50)
    const w = mountCard(textPreview({ truncated: false }))
    await flush()
    await w.find('.pv-toggle').trigger('click')
    expect(w.find('.pv-text').classes()).toContain('full')
    expect(w.find('.pv-toggle').text()).toContain('收起')

    // 展开态正文高度 < 220px 时 scrollHeight == clientHeight，模拟 resize 触发重算
    setLayout(100, 100)
    window.dispatchEvent(new Event('resize'))
    await flush()
    expect(w.find('.pv-toggle').exists()).toBe(true)
    expect(w.find('.pv-toggle').text()).toContain('收起')
    expect(w.find('.pv-text').classes()).toContain('full')
  })
})

// ------------------------------------- BUG-005：已确认卡片布局加固
describe('BUG-005 · 已确认卡片布局加固', () => {
  const longPath = 'D:/归档/很深的一级目录/二级目录/三级目录/四级目录/最终文件.docx'

  function confirmedCard(over: Partial<BatchItem['file']> = {}) {
    return mount(FileCard, {
      props: {
        item: item({ status: 'confirmed', absPath: longPath, ...over }),
        dirCount: 2,
        position: { index: 0, total: 3 },
        preview: null,
        ack: {
          fileId: 1,
          status: 'confirmed',
          confirmed: 1,
          targetRelPath: longPath,
          remainingInPool: 3,
        },
      },
    })
  }

  it('confirmed：根节点带 done，候选区与判定按钮不渲染，只留删除入口，结果行给长路径 title', () => {
    const w = confirmedCard()
    expect(w.find('.card').classes()).toContain('done')
    // 已确认卡片不再渲染候选区与「确认/暂缓/先放着」按钮；
    // BUG-007 起保留一个「删除（移入隔离区）」入口
    expect(w.find('.body').find('*').exists()).toBe(false)
    expect(w.findAll('.ft .btn')).toHaveLength(1)
    expect(w.find('.ft .btn').text()).toContain('移入隔离区')
    expect(w.find('.ft > .hint').exists()).toBe(true)
    // 结果行的长路径必须有完整 title（这是加固项：原来直接塞在 <b> 里，没有 title）
    const dest = w.find('.result > .dest')
    expect(dest.exists()).toBe(true)
    expect(dest.attributes('title')).toBe(longPath)
    expect(dest.text()).toContain(longPath)
  })

  it('长路径结果行最多两行后省略，不靠换行把绿色条撑高', () => {
    const css = cssRule(fileCardSource, '.result .dest')
    expect(css, '.dest 必须限制溢出').toMatch(/overflow:\s*hidden/)
    expect(css, '.dest 必须用 -webkit-box 做两行裁切').toMatch(/-webkit-box-orient:\s*vertical/)
    expect(css, '.dest 最多两行').toMatch(/-webkit-line-clamp:\s*2/)
    expect(css, '长路径内部允许断开，避免撑破容器').toMatch(/overflow-wrap:\s*anywhere/)
    // 外层结果行也必须是可收缩的 flex 子项
    expect(cssRule(fileCardSource, '.result')).toMatch(/min-width:\s*0/)
  })

  it('标题区与徽章区分列且各自可收缩：多徽章换行不覆盖文件名/路径', () => {
    const w = confirmedCard()
    // DOM 结构上三者互不嵌套，徽章再多也不会长进标题块里
    expect(w.find('.hd > .nm').exists()).toBe(true)
    expect(w.find('.hd > .state').exists()).toBe(true)
    expect(w.find('.hd > .nm > .nameline > .name').exists()).toBe(true)
    expect(w.find('.hd > .state > .badge.ok').exists()).toBe(true)

    const hd = cssRule(fileCardSource, '.hd')
    expect(hd, '.hd 用 grid 分列，minmax(0,1fr) 让标题可收缩而不被徽章顶破').toMatch(
      /grid-template-columns:[^;]*minmax\(0,\s*1fr\)/,
    )
    const state = cssRule(fileCardSource, '.state')
    expect(state, '徽章区需要 max-width 才能在窄窗换行').toMatch(/max-width:\s*\d+%/)
    expect(state, '徽章区需要 min-width:0 才能作为 grid 子项收缩').toMatch(/min-width:\s*0/)
    expect(state, '徽章换行后要有行间距').toMatch(/row-gap:\s*\d+px/)
  })

  it('标题与徽章之间保留 ≥8px 的最小水平间距', () => {
    const hd = cssRule(fileCardSource, '.hd')
    const gap = hd.match(/column-gap:\s*(\d+(?:\.\d+)?)px/)
    expect(gap, '.hd 必须显式声明 column-gap，不能只写 gap').toBeTruthy()
    expect(parseFloat(gap![1])).toBeGreaterThanOrEqual(8)
    expect(hd, '换行时也要有行间距').toMatch(/row-gap:\s*\d+px/)
  })

  it('min-height:0 只给真正的滚动容器，不给被压缩的卡片子项', () => {
    // 项目踩坑：待迁移曾被压扁溢出。卡片和 .body 都不是滚动容器，不许写 min-height:0
    expect(cssRule(fileCardSource, '.card')).not.toMatch(/min-height:\s*0/)
    expect(cssRule(fileCardSource, '.body')).not.toMatch(/min-height:\s*0/)
  })

  it('长文件名仍单行省略且 title 为完整值（ confirmed 态不退化）', () => {
    const longName = '2026年度智能工厂平台安全生产大检查暨消防设施专项验收工作情况汇报最终定稿v3.docx'
    const w = confirmedCard({ name: longName })
    const name = w.find('.hd > .nm > .nameline > .name')
    expect(name.text()).toBe(longName)
    expect(name.attributes('title')).toBe(longName)
    const nameCss = cssRule(fileCardSource, '.name')
    expect(nameCss).toMatch(/text-overflow:\s*ellipsis/)
    expect(nameCss).toMatch(/white-space:\s*nowrap/)
  })
})

/* ---- BUG-007：待确认卡片 / 已确认条目都要有「删除（移入隔离区）」入口 ---- */
describe('BUG-007：删除入口', () => {
  function mountCard(over: Partial<BatchItem['file']> = {}) {
    return mount(FileCard, {
      props: {
        item: item(over),
        dirCount: 2,
        position: { index: 0, total: 1 },
      },
    })
  }

  it('待确认卡片有删除按钮，点击 emit trash', async () => {
    const w = mountCard()
    const btn = w.findAll('button').find((b) => /删除|隔离区/.test(b.text()))
    expect(btn, '待确认卡片必须有删除入口').toBeTruthy()
    expect(btn!.text()).toContain('移入隔离区')
    await btn!.trigger('click')
    expect(w.emitted('trash')).toBeTruthy()
  })

  it('已确认卡片同样有删除入口，文案禁止「永久删除」', () => {
    const w = mountCard({ status: 'confirmed' })
    const btn = w.findAll('button').find((b) => /删除|隔离区/.test(b.text()))
    expect(btn, '已确认条目也必须有删除入口').toBeTruthy()
    expect(w.text()).not.toContain('永久删除')
  })
})
