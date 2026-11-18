// @vitest-environment happy-dom
/**
 * 确认页（左树 + 右清单）的渲染测试。
 *
 * 上一版这块是纯手写模板，没有任何测试，结果是「看不清晰」——只能说没法用眼睛验证的地方
 * 就得让测试替眼睛看：树上有哪些节点、点一下右边是不是真的换了清单。
 */
import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { useWizardStore } from '../src/stores/wizard'
import TopicConfirmStep from '../src/pages/wizard/TopicConfirmStep.vue'

const proposal = () => ({
  topics: [
    {
      id: 't1',
      name: '人工智能推进会',
      aliases: [],
      fileIds: ['f1', 'f2'],
      samples: [],
      domain: '项目建设',
    },
    { id: 't2', name: '数智化项目建设', aliases: [], fileIds: ['f3'], samples: [], domain: '项目建设' },
    { id: 't3', name: '技术谱系', aliases: [], fileIds: ['f4'], samples: [], domain: '技术研发' },
    { id: 't4', name: '软件开发', aliases: [], fileIds: ['f5'], samples: [], domain: '技术研发' },
    { id: 't5', name: '境内差旅费', aliases: [], fileIds: ['f6'], samples: [] },
    {
      id: 't6',
      name: '演讲稿',
      aliases: [],
      fileIds: ['f7'],
      samples: [],
      weak: true,
      weakReason: 'doc' as const,
    },
  ],
  ungroupedIds: ['u1', 'u2'],
})

function setup() {
  setActivePinia(createPinia())
  const w = useWizardStore()
  w.proposal = proposal()
  w.domainSource = 'llm'
  w.fileIndex = {
    f1: '人工智能推进会纪要.docx',
    f2: '人工智能推进会材料.pptx',
    f3: '数智化建设方案.docx',
    f4: '技术谱系V1.3.docx',
    f5: '软件开发说明.docx',
    f6: '境内差旅费报销单.xlsx',
    f7: '演讲稿.docx',
    u1: '未命名文档1.docx',
    u2: '未命名文档2.docx',
  }
  w.filePathIndex = {
    f1: 'D:\\未整理\\会议\\人工智能推进会纪要.docx',
    f2: 'D:\\未整理\\会议\\人工智能推进会材料.pptx',
    u1: 'D:\\未整理\\杂项\\未命名文档1.docx',
    u2: 'D:\\未整理\\杂项\\未命名文档2.docx',
  }
  return { w, wrapper: mount(TopicConfirmStep) }
}

describe('左树', () => {
  it('大类全部显示，事项挂在下面', () => {
    const { wrapper } = setup()
    const text = wrapper.text()
    expect(text).toContain('项目建设')
    expect(text).toContain('技术研发')
    expect(text).toContain('人工智能推进会')
    expect(text).toContain('技术谱系')
  })

  it('大类按文件数降序——份数多的更容易被看见', () => {
    const { wrapper } = setup()
    const doms = wrapper.findAll('[data-key^="d:"]').map((n) => n.text())
    expect(doms[0]).toContain('项目建设')
  })

  it('没有大类的事项单独列在「不在大类里」，兜底类和未归类也各有入口', () => {
    const { wrapper } = setup()
    const text = wrapper.text()
    expect(text).toContain('不在大类里')
    expect(text).toContain('境内差旅费')
    expect(text).toContain('待定')
    expect(text).toContain('未归类')
  })

  it('标题说明分了几个大类', () => {
    const { wrapper } = setup()
    expect(wrapper.text()).toContain('2 个大类')
  })
})

describe('右清单', () => {
  it('默认选中第一个大类，列出它名下所有事项的文件', () => {
    const { wrapper } = setup()
    const text = wrapper.text()
    expect(text).toContain('人工智能推进会纪要.docx')
    expect(text).toContain('数智化建设方案.docx')
    expect(text).not.toContain('技术谱系V1.3.docx')
  })

  it('点事项，右边只留这个事项的文件', async () => {
    const { wrapper } = setup()
    const row = wrapper.findAll('[data-key="t:t2"]')[0]
    await row.trigger('click')
    const text = wrapper.text()
    expect(text).toContain('数智化建设方案.docx')
    expect(text).not.toContain('人工智能推进会纪要.docx')
  })

  it('点未归类，按来源文件夹分组显示', async () => {
    const { wrapper } = setup()
    await wrapper.findAll('[data-key="un"]')[0].trigger('click')
    const text = wrapper.text()
    expect(text).toContain('未命名文档1.docx')
    expect(text).toContain('D:\\未整理\\杂项')
  })

  it('点待定，能看到兜底类名和它的文件', async () => {
    const { wrapper } = setup()
    await wrapper.findAll('[data-key="weak"]')[0].trigger('click')
    expect(wrapper.text()).toContain('演讲稿.docx')
  })

  it('域名可点改名（进编辑态而不是直接跳走）', async () => {
    const { wrapper } = setup()
    await wrapper.find('.tc-name-btn').trigger('click')
    expect(wrapper.find('.tc-edit').exists()).toBe(true)
  })
})

describe('工具栏', () => {
  it('选中大类时给的是「改名 / 解散大类」，不是事项的删除', () => {
    const { wrapper } = setup()
    const acts = wrapper.find('.tc-acts').text()
    expect(acts).toContain('解散大类')
    expect(acts).not.toContain('删除')
  })

  it('解散大类后，它的事项回到「不在大类里」，文件一个不少', async () => {
    const { w, wrapper } = setup()
    const btn = wrapper.findAll('.tc-acts button').find((b) => b.text() === '解散大类')!
    await btn.trigger('click')
    expect(w.domainNames).toEqual(['技术研发'])
    expect(wrapper.text()).toContain('不在大类里')
    expect(w.proposalTopics.reduce((n, t) => n + t.fileIds.length, 0)).toBe(7)
  })

  it('事项可以挪到别的大类', async () => {
    const { w, wrapper } = setup()
    await wrapper.findAll('[data-key="t:t3"]')[0].trigger('click')
    const move = wrapper.findAll('.tc-acts button').find((b) => b.text() === '移到…')!
    await move.trigger('click')
    const target = wrapper.findAll('.tc-move-pop button').find((b) => b.text() === '项目建设')!
    await target.trigger('click')
    expect(w.proposal!.topics.find((t) => t.id === 't3')!.domain).toBe('项目建设')
  })

  it('没有分类时也能渲染空状态，不白屏', () => {
    setActivePinia(createPinia())
    const w = useWizardStore()
    w.proposal = { topics: [], ungroupedIds: [] }
    const wrapper = mount(TopicConfirmStep)
    expect(wrapper.text()).toContain('0 份')
  })
})
