// @vitest-environment happy-dom
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import FileList from '../src/pages/wizard/FileList.vue'
import { useWizardStore } from '../src/stores/wizard'

const NAMES: Record<string, string> = {
  a: '技术方案V1.docx',
  b: '技术方案V2.docx',
  c: '技术方案v3.docx',
  d: '完全无关.pdf',
  e: '另一个方案(1).docx',
  f1: 'ppt修改.pptx',
  f2: 'ppt修改.pptx',
}

function setup(ids: string[], showPath = true) {
  setActivePinia(createPinia())
  const w = useWizardStore()
  w.fileIndex = { ...NAMES }
  w.filePathIndex = Object.fromEntries(ids.map((id) => [id, `D:\\源\\${NAMES[id]}`]))
  w.fileMeta = {
    a: { size: 1024, mtime: new Date('2024-01-01').getTime() },
    b: { size: 2 * 1024 * 1024, mtime: new Date('2024-03-01').getTime() },
    c: { size: 3 * 1024 * 1024, mtime: new Date('2024-05-01').getTime() },
    d: { size: 500, mtime: new Date('2024-05-01').getTime() },
    e: { size: 800, mtime: new Date('2024-05-01').getTime() },
    f1: { size: 700, mtime: new Date('2024-05-02').getTime() },
    f2: { size: 600, mtime: new Date('2024-04-02').getTime() },
  }
  return mount(FileList, { props: { ids, showPath } })
}

describe('文件清单：版本族折叠', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('同一份文档的多个版本只占一行，写明几个版本和最新大小', () => {
    const text = setup(['a', 'b', 'c']).text()
    expect(text).toContain('技术方案')
    expect(text).toContain('3 个版本')
    expect(text).toContain('3.0MB') // 最新那份 c
    // 折叠状态下不该把每个版本名都摊出来
    expect(text).not.toContain('技术方案V1.docx')
  })

  it('点开后列出每一版，并标出最新/旧版', async () => {
    const wrapper = setup(['a', 'b', 'c'])
    await wrapper.find('.fl-head').trigger('click')
    const text = wrapper.text()
    expect(text).toContain('技术方案v3.docx')
    expect(text).toContain('技术方案V1.docx')
    expect(text).toContain('最新')
    expect(text).toContain('旧版')
  })

  it('单份文件照常一行显示，路径也在', () => {
    const text = setup(['d']).text()
    expect(text).toContain('完全无关.pdf')
    expect(text).toContain('D:\\源\\完全无关.pdf')
    expect(text).not.toContain('个版本')
  })

  it('同名副本写成「N 份同名」而不是「N 个版本」', () => {
    const text = setup(['f1', 'f2']).text()
    expect(text).toContain('2 份同名')
    expect(text).not.toContain('个版本')
  })

  it('预览页不需要重复展示源路径', () => {
    expect(setup(['d'], false).text()).not.toContain('D:\\源')
  })

  it('内容完全相同的副本会在行上标出来', () => {
    const wrapper = setup(['d'])
    const w = useWizardStore()
    w.dupIndex = { d: ['x'] }
    return wrapper.vm.$nextTick().then(() => {
      expect(wrapper.text()).toContain('与另 1 份内容相同')
    })
  })

  it('不同主名的文件不会被折进同一行', () => {
    const text = setup(['a', 'd']).text()
    expect(text).toContain('技术方案V1.docx')
    expect(text).toContain('完全无关.pdf')
  })
})
