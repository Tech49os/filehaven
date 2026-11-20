import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import type { BatchItem, FilePreview } from '../core/types'

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

  it('有正文时直接显示摘要，长得截断的给「展开」', async () => {
    const w = mountCard(textPreview({ truncated: true }))
    expect(w.text()).toContain('合同金额：十二万元')
    const toggle = w.find('.pv-toggle')
    expect(toggle.exists()).toBe(true)
    expect(toggle.text()).toContain('展开')
    await toggle.trigger('click')
    expect(toggle.text()).toContain('收起')
    expect(w.find('.pv-text').classes()).toContain('full')
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
})
