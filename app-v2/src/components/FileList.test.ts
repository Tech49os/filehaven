import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import type { FileRow } from '../core/types'
import fileList from './FileList.vue?raw'
import browser from '../pages/browser/Browser.vue?raw'
import tokens from '../styles/tokens.css?raw'

const FileList = (await import('./FileList.vue')).default

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
    archivedPath: null,
    archiveDirId: null,
    tags: [
      { id: 3, name: '安全检查', kind: 'topic' },
      { id: 5, name: '重点', kind: 'user' },
    ],
    ...over,
  }
}

/** 只取 <style> 块，避免模板里的字面量干扰「不许硬编码字号」判断 */
function styleBlock(src: string): string {
  return src.match(/<style[^>]*>([\s\S]*?)<\/style>/)?.[1] ?? ''
}

describe('浏览页文件列表：列结构与交互（回归守卫）', () => {
  it('每行仍是六格：名称 / 类型 / 大小 / 时间 / 标签 / 状态', () => {
    const w = mount(FileList, { props: { rows: [file()] } })
    const cells = w.find('.row').findAll('.cell')
    expect(cells).toHaveLength(6)
    expect(cells[0].text()).toContain('安全检查通知.docx')
    expect(cells[1].text()).toBeTruthy()
    expect(cells[5].text()).toBeTruthy()
  })

  it('单击 / 双击 / 右键各自发事件，不因样式调整而改变', async () => {
    const w = mount(FileList, { props: { rows: [file()] } })
    const row = w.find('.row')
    await row.trigger('click')
    await row.trigger('dblclick')
    await row.trigger('contextmenu')
    expect(w.emitted('activate')?.[0]).toEqual([1])
    expect(w.emitted('open')?.[0]).toEqual([file()])
    expect(w.emitted('menu')?.[0]?.[0]).toMatchObject({ row: file() })
  })

  it('标签 chip 点击发 tagClick，并阻止冒泡到整行', async () => {
    const w = mount(FileList, { props: { rows: [file()] } })
    await w.find('.tchip').trigger('click')
    expect(w.emitted('tagClick')?.[0]?.[0]).toMatchObject({ tagId: 3 })
    expect(w.emitted('activate')).toBeUndefined()
  })
})

describe('BUG-009 · 字号与行高走 BUG-008 token', () => {
  const css = styleBlock(fileList)

  it('文件名 / 次要列 / chip / 行高分别引用 --fs-file-name / --fs-meta / --fs-chip / --lh-file', () => {
    expect(css, '文件名走 --fs-file-name').toContain('font-size: var(--fs-file-name)')
    expect(css, '次要列走 --fs-meta').toContain('font-size: var(--fs-meta)')
    expect(css, '标签 chip 走 --fs-chip').toContain('font-size: var(--fs-chip)')
    expect(css, '行高走 --lh-file').toContain('line-height: var(--lh-file)')
  })

  it('style 块内没有任何硬编码字号（旧 12.5 / 11.5 / 10px 必须消失）', () => {
    expect(css).not.toMatch(/font-size:\s*\d+(?:\.\d+)?px/)
  })

  it('行内边距与间隙引用 --pad-row-x / --pad-row-y / --gap-row', () => {
    expect(css).toContain('var(--pad-row-x)')
    expect(css).toContain('var(--pad-row-y)')
    expect(css).toContain('var(--gap-row)')
  })

  it('行高由内容决定且不低于 36px（没有固定 32px 高度）', () => {
    expect(css).toMatch(/min-height:\s*36px/)
    expect(css).not.toMatch(/(^|[^-])height:\s*32px/)
  })
})

describe('BUG-009 · 删除 Browser.vue 重复死样式', () => {
  it('Browser.vue 不再含 .tchip 规则块', () => {
    expect(browser).not.toMatch(/\.tchip\s*\{/)
  })

  it('Browser.vue 不再含重复的 .row / .cell 列表样式块', () => {
    expect(browser).not.toMatch(/\.row\s*\{[^}]*grid-template-columns/)
    expect(browser).not.toMatch(/\.cell\.name\s*\{/)
  })
})

describe('BUG-009 · 窄窗策略写进 tokens.css', () => {
  it('明确采用横向滚动或隐藏次要列，不靠缩字号硬塞 6 列', () => {
    expect(tokens).toMatch(/BUG-009/)
    expect(tokens).toMatch(/横向滚动|隐藏次要列/)
  })
})
