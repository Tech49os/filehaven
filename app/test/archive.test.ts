import { describe, it, expect } from 'vitest'
import {
  ARCHIVE_VERSION,
  buildProposalFromSaved,
  fromSavedPlan,
  toSavedPlan,
  type SavedPlan,
} from '../src/core/emergence/archive'
import type { Proposal } from '../src/core/emergence/proposal'
import type { FileItem } from '../src/core/types'

const F = (id: string, name: string, text = ''): FileItem =>
  ({
    id,
    name,
    ext: '.docx',
    kind: 'doc',
    size: 1024,
    mtime: 0,
    path: `D:\\未整理\\${name}`,
    textSummary: text,
    categoryId: null,
    suggestedName: '',
    tags: [],
    confidence: 0,
    needReview: true,
    status: 'scanned',
  }) as FileItem

const P = (): Proposal => ({
  topics: [
    {
      id: 't1',
      name: '技术谱系',
      aliases: ['技术谱系'],
      fileIds: ['a'],
      samples: ['技术谱系汇报.docx'],
      domain: '技术研发',
    },
    { id: 't2', name: '演讲稿', aliases: ['演讲稿'], fileIds: ['b'], samples: [], weak: true, weakReason: 'doc' },
  ],
  ungroupedIds: ['c'],
})

describe('存：把确认过的方案压成可持久化的形状', () => {
  it('带版本号、归档根目录和保存时间', () => {
    const s = toSavedPlan(P(), 'D:\\文档归档', 1000)
    expect(s.version).toBe(ARCHIVE_VERSION)
    expect(s.root).toBe('D:\\文档归档')
    expect(s.savedAt).toBe(1000)
  })

  it('只留类名/别名/大域/降级原因，不留文件 id（下次扫描 id 全变）', () => {
    const s = toSavedPlan(P(), 'D:\\文档归档', 1000)
    expect(s.topics[0]).toEqual({
      name: '技术谱系',
      aliases: ['技术谱系'],
      domain: '技术研发',
      weak: undefined,
      weakReason: undefined,
    })
    expect(JSON.stringify(s)).not.toContain('技术谱系汇报.docx')
  })

  it('大域跟着存档走，下次沿用时目录还是两层的', () => {
    const saved = toSavedPlan(P(), 'D:\\a', 1000)
    const back = fromSavedPlan(saved)!
    expect(back[0].domain).toBe('技术研发')
    // 兜底类没有域，回到第一层
    expect(back[1].domain).toBeUndefined()
  })

  it('旧版（v1）存档照样能读，只是没有大类', () => {
    const v1 = toSavedPlan(P(), 'D:\\a')
    // v1 存档里根本没有 domain 这个字段
    for (const t of v1.topics) delete (t as { domain?: string }).domain
    const back = fromSavedPlan({ ...v1, version: 1 })!
    expect(back.length).toBe(2)
    expect(back[0].name).toBe('技术谱系')
    expect(back[0].domain).toBeUndefined()
  })

  it('域字段是脏数据（数字/空串）时不炸，当成没有', () => {
    const s = toSavedPlan(P(), 'D:\\a')
    ;(s.topics[0] as unknown as { domain: unknown }).domain = 123
    const back = fromSavedPlan(s)!
    expect(back[0].domain).toBeUndefined()
  })

  it('改名后的类名被记住，别名保留原始写法', () => {
    const p = P()
    p.topics[0].name = 'AI技术谱系'
    const s = toSavedPlan(p, 'D:\\a', 1000)
    expect(s.topics[0].name).toBe('AI技术谱系')
    expect(s.topics[0].aliases).toEqual(['技术谱系'])
  })
})

describe('取：存档坏了不能把流程带崩', () => {
  it('版本不匹配直接当没有', () => {
    expect(fromSavedPlan({ ...toSavedPlan(P(), 'D:\\a'), version: 999 })).toBeNull()
  })

  it('空方案当没有', () => {
    expect(fromSavedPlan({ ...toSavedPlan(P(), 'D:\\a'), topics: [] })).toBeNull()
  })

  it('null / undefined 当没有', () => {
    expect(fromSavedPlan(null)).toBeNull()
    expect(fromSavedPlan(undefined)).toBeNull()
  })

  it('类名缺失的条目跳过，其余照用', () => {
    const bad = {
      ...toSavedPlan(P(), 'D:\\a'),
      topics: [{ name: '  ', aliases: ['x'] }, { name: '技术谱系', aliases: [] }],
    } as unknown as SavedPlan
    const topics = fromSavedPlan(bad)!
    expect(topics).toHaveLength(1)
    expect(topics[0].name).toBe('技术谱系')
    // 别名缺失时退回类名，否则这个类永远匹配不到文件
    expect(topics[0].aliases).toEqual(['技术谱系'])
  })

  it('取出来重新分配 id，不会和本轮新生成的撞车', () => {
    const topics = fromSavedPlan(toSavedPlan(P(), 'D:\\a'))!
    expect(topics.map((t) => t.id)).toEqual(['s1', 's2'])
  })
})

describe('用：沿用存档立刻得到提案，不需要跑模型', () => {
  it('按类名/别名重新认领文件', () => {
    const files = [
      F('1', '技术谱系梳理.docx'),
      F('2', '演讲稿(1).docx'),
      F('3', '无关文件.docx'),
    ]
    const p = buildProposalFromSaved(files, toSavedPlan(P(), 'D:\\a'))!
    expect(p.topics.find((t) => t.name === '技术谱系')!.fileIds).toEqual(['1'])
    expect(p.topics.find((t) => t.name === '演讲稿')!.fileIds).toEqual(['2'])
    expect(p.ungroupedIds).toEqual(['3'])
  })

  it('存档里的空类不会带进来（沿用不是复制僵尸目录）', () => {
    const files = [F('1', '技术谱系梳理.docx')]
    const p = buildProposalFromSaved(files, toSavedPlan(P(), 'D:\\a'))!
    expect(p.topics.map((t) => t.name)).toEqual(['技术谱系'])
  })

  it('降级原因要带过来，界面才知道该折叠', () => {
    const files = [F('1', '技术谱系梳理.docx'), F('2', '演讲稿(1).docx')]
    const p = buildProposalFromSaved(files, toSavedPlan(P(), 'D:\\a'))!
    const weak = p.topics.find((t) => t.name === '演讲稿')!
    expect(weak.weak).toBe(true)
    expect(weak.weakReason).toBe('doc')
  })

  it('存档里的类一个文件都没匹配到，返回的提案里就没有类', () => {
    const p = buildProposalFromSaved([F('1', '别人的文件.docx')], toSavedPlan(P(), 'D:\\a'))!
    expect(p.topics).toHaveLength(0)
    expect(p.ungroupedIds).toEqual(['1'])
  })
})
