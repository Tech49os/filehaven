import { describe, it, expect } from 'vitest'
import { applyProposal, buildProposal, reassign, type ProposalTopic } from '../src/core/emergence/proposal'
import type { FileItem } from '../src/core/types'
import type { TopicGroup } from '../src/core/emergence/grouping'

const F = (id: string, name: string, body = ''): FileItem =>
  ({
    id,
    name,
    ext: name.split('.').pop() ?? '',
    kind: 'doc',
    size: 1000,
    mtime: Date.now(),
    path: name,
    textSummary: body,
    categoryId: null,
    suggestedName: name,
    tags: [],
    confidence: 0,
    needReview: false,
    status: 'scanned',
  }) as FileItem

const G = (name: string, aliases: string[]): TopicGroup => ({ name, aliases, weight: 10 })

describe('提案装配：文件归属', () => {
  it('文件名里出现别名就归入该事项', () => {
    const files = [F('1', '技术谱系-初稿.docx'), F('2', '无关的东西.docx')]
    const p = buildProposal(files, [G('技术谱系', ['技术谱系'])])
    expect(p.topics[0].fileIds).toEqual(['1'])
    expect(p.ungroupedIds).toEqual(['2'])
  })

  it('正文里出现别名也能归入（置信较低但仍归入）', () => {
    const files = [F('1', 'final_v2.docx', '本文梳理技术谱系与智能感知应用的落地方案，技术谱系覆盖边缘侧与平台侧')]
    const p = buildProposal(files, [G('技术谱系', ['技术谱系'])])
    expect(p.topics[0].fileIds).toContain('1')
  })

  it('多个别名同时命中时归给更具体（更长）的那个', () => {
    const files = [F('1', '人工智能推进会-8月4日.pptx')]
    const p = buildProposal(files, [
      G('人工智能', ['人工智能']),
      G('人工智能推进会', ['人工智能推进会']),
    ])
    const target = p.topics.find((t) => t.name === '人工智能推进会')!
    expect(target.fileIds).toContain('1')
    // 泛词事项没有独占任何文件，不该出现在提案里（否则用户看到的是一堆空类）
    expect(p.topics.find((t) => t.name === '人工智能')).toBeUndefined()
  })

  it('文件总数守恒：归入的 + 未归类的 = 总数', () => {
    const files = [F('1', '技术谱系.docx'), F('2', '不知道是什么.bin'), F('3', '梯度培育培训.docx')]
    const p = buildProposal(files, [G('技术谱系', ['技术谱系']), G('梯度培育培训', ['梯度培育培训'])])
    const assigned = p.topics.reduce((n, t) => n + t.fileIds.length, 0)
    expect(assigned + p.ungroupedIds.length).toBe(files.length)
  })

  it('每个事项带代表文件名，最多 5 个', () => {
    const files = Array.from({ length: 8 }, (_, i) => F(String(i), `技术谱系-${i}.docx`))
    const p = buildProposal(files, [G('技术谱系', ['技术谱系'])])
    expect(p.topics[0].samples.length).toBeLessThanOrEqual(5)
    expect(p.topics[0].samples.length).toBeGreaterThan(0)
  })

  it('空事项不出现在提案里', () => {
    const files = [F('1', '技术谱系.docx')]
    const p = buildProposal(files, [G('技术谱系', ['技术谱系']), G('不存在的事', ['根本没有这个词'])])
    expect(p.topics).toHaveLength(1)
    expect(p.topics[0].name).toBe('技术谱系')
  })
})

describe('确认后再算：改名 / 删除 / 合并', () => {
  const files = [
    F('1', '技术谱系-甲.docx'),
    F('2', '智能感知应用-乙.docx'),
    F('3', '梯度培育培训-丙.docx'),
    F('4', '完全无关.txt'),
  ]
  const topics: ProposalTopic[] = [
    { id: 't1', name: '技术谱系', aliases: ['技术谱系', '智能感知应用'], fileIds: [], samples: [] },
    { id: 't2', name: '梯度培育培训', aliases: ['梯度培育培训'], fileIds: [], samples: [] },
  ]

  it('重算后归属正确', () => {
    const p = reassign(files, topics)
    expect(p.topics.find((t) => t.id === 't1')!.fileIds.sort()).toEqual(['1', '2'])
    expect(p.topics.find((t) => t.id === 't2')!.fileIds).toEqual(['3'])
    expect(p.ungroupedIds).toEqual(['4'])
  })

  it('删掉一个事项后，它的文件回到未归类，不会消失', () => {
    const p = reassign(files, [topics[0]])
    expect(p.ungroupedIds.sort()).toEqual(['3', '4'])
  })

  it('改名不影响归属（别名没变）', () => {
    const renamed = [{ ...topics[0], name: '技术谱系梳理' }, topics[1]]
    const p = reassign(files, renamed)
    expect(p.topics.find((t) => t.id === 't1')!.fileIds.sort()).toEqual(['1', '2'])
  })

  it('合并后两类的文件并到一类', () => {
    const merged: ProposalTopic = {
      id: 't1',
      name: '技术谱系',
      aliases: ['技术谱系', '智能感知应用', '梯度培育培训'],
      fileIds: [],
      samples: [],
    }
    const p = reassign(files, [merged])
    expect(p.topics[0].fileIds.sort()).toEqual(['1', '2', '3'])
  })
})

describe('兜底类（只是文种的说法）只捡剩下的文件', () => {
  it('真事项认领的文件不会被文种类抢走', () => {
    const files = [
      F('1', '人工智能推进会-会议材料.pptx'),
      F('2', '演讲稿(1).docx'),
    ]
    const groups: TopicGroup[] = [
      { name: '人工智能推进会', aliases: ['人工智能推进会'], weight: 10, weak: false },
      { name: '演讲稿', aliases: ['演讲稿'], weight: 5, weak: true },
    ]
    const p = buildProposal(files, groups)
    expect(p.topics.find((t) => t.name === '人工智能推进会')!.fileIds).toEqual(['1'])
    expect(p.topics.find((t) => t.name === '演讲稿')!.fileIds).toEqual(['2'])
  })

  it('文种类内文提到真事项时，仍然留在文种类，不被吸走', () => {
    const files = [
      F('1', 'ppt修改.pptx', '人工智能推进会 汇报内容 人工智能推进会 后续安排'),
      F('2', '人工智能推进会纪要.docx'),
    ]
    const groups: TopicGroup[] = [
      { name: '人工智能推进会', aliases: ['人工智能推进会'], weight: 10, weak: false },
      { name: '修改', aliases: ['修改'], weight: 5, weak: true },
    ]
    const p = buildProposal(files, groups)
    // 文件名命中 > 正文命中，所以 ppt修改 不会被正文拽走
    expect(p.topics.find((t) => t.name === '修改')!.fileIds).toEqual(['1'])
  })

  it('文种类没有独有文件时自动消失', () => {
    const files = [F('1', '人工智能推进会纪要.docx')]
    const groups: TopicGroup[] = [
      { name: '人工智能推进会', aliases: ['人工智能推进会'], weight: 10, weak: false },
      { name: '进展汇报', aliases: ['进展汇报'], weight: 5, weak: true },
    ]
    const p = buildProposal(files, groups)
    expect(p.topics.map((t) => t.name)).toEqual(['人工智能推进会'])
  })
})

describe('落地：树 → 分类目录', () => {
  const files = [F('a', '技术谱系汇报.docx'), F('b', '技术谱系附件.docx'), F('c', '差旅费.xlsx')]
  const P = () => ({
    topics: [
      { id: 't1', name: '技术谱系', aliases: [], fileIds: ['a'], samples: [], domain: '技术研发' },
      { id: 't2', name: '技术沉淀', aliases: [], fileIds: ['b'], samples: [], domain: '技术研发' },
      { id: 't3', name: '境内差旅费', aliases: [], fileIds: ['c'], samples: [] },
    ],
    ungroupedIds: [],
  })

  it('有大类的事项建成两层目录：先父后子，parentId 指对了', () => {
    const r = applyProposal(files, P())
    const root = r.categories.find((c) => !c.parentId)!
    expect(root.name).toBe('技术研发')
    const kids = r.categories.filter((c) => c.parentId === root.id)
    expect(kids.map((c) => c.name).sort()).toEqual(['技术谱系', '技术沉淀'].sort())
  })

  it('没有大类的事项直接挂在归档根目录下', () => {
    const r = applyProposal(files, P())
    const loose = r.categories.find((c) => c.name === '境内差旅费')!
    expect(loose.parentId).toBeNull()
  })

  it('文件的 categoryId 指向自己的事项，不是父目录', () => {
    const r = applyProposal(files, P())
    const idOf = (name: string) => r.categories.find((c) => c.name === name)!.id
    expect(r.files.find((f) => f.id === 'a')!.categoryId).toBe(idOf('技术谱系'))
    expect(r.files.find((f) => f.id === 'c')!.categoryId).toBe(idOf('境内差旅费'))
  })

  it('只有自己的域不建父层——单孩子目录只是多一层点击', () => {
    const r = applyProposal(files, {
      topics: [
        { id: 't1', name: '技术谱系', aliases: [], fileIds: ['a'], samples: [], domain: '技术研发' },
        { id: 't2', name: '境内差旅费', aliases: [], fileIds: ['c'], samples: [] },
      ],
      ungroupedIds: [],
    })
    expect(r.categories.every((c) => !c.parentId)).toBe(true)
    expect(r.categories.map((c) => c.name).sort()).toEqual(['境内差旅费', '技术谱系'])
  })

  it('分类 id 不重复（父子混编也不会撞）', () => {
    const r = applyProposal(files, P())
    expect(new Set(r.categories.map((c) => c.id)).size).toBe(r.categories.length)
  })
})
