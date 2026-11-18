import { describe, it, expect } from 'vitest'
import { buildPlans, sanitizeSegment } from '../src/core/plan'
import type { Category, FileItem } from '../src/core/types'

const C = (id: string, name: string, parentId: string | null = null): Category => ({
  id,
  name,
  desc: '',
  parentId,
  order: 0,
  isFromAI: true,
})

const F = (id: string, name: string, opts: Partial<FileItem> = {}): FileItem =>
  ({
    id,
    name,
    ext: name.includes('.') ? name.slice(name.lastIndexOf('.')) : '',
    kind: 'doc',
    size: 1024,
    mtime: Date.now(),
    path: `D:\\源目录\\${name}`,
    textSummary: '',
    categoryId: null,
    suggestedName: '',
    tags: [],
    confidence: 0,
    needReview: false,
    status: 'scanned',
    ...opts,
  }) as FileItem

describe('目录名清洗（与 Rust sanitize_segment 保持一致）', () => {
  it('Windows 非法字符换成下划线', () => {
    expect(sanitizeSegment('A/B:C*D?E"F<G>H|I\\J')).toBe('A_B_C_D_E_F_G_H_I_J')
  })

  it('去掉首尾空格和结尾的点', () => {
    expect(sanitizeSegment('  技术谱系  ')).toBe('技术谱系')
    expect(sanitizeSegment('技术谱系.')).toBe('技术谱系')
  })

  it('正常中文名不受影响', () => {
    expect(sanitizeSegment('钻采院高温高压项目')).toBe('钻采院高温高压项目')
  })
})

describe('迁移计划：目录归属', () => {
  it('单层分类直接拿类名当目录', () => {
    const plans = buildPlans([F('1', 'a.docx', { categoryId: 'c1' })], [C('c1', '技术谱系')])
    expect(plans[0].targetDir).toBe('技术谱系')
    expect(plans[0].deferred).toBe(false)
  })

  it('两层分类拼成 父\\子', () => {
    const cats = [C('c1', '部门管理'), C('c2', '会议纪要', 'c1')]
    const plans = buildPlans([F('1', 'a.docx', { categoryId: 'c2' })], cats)
    expect(plans[0].targetDir).toBe('部门管理\\会议纪要')
  })

  it('没归类的文件不迁移', () => {
    const plans = buildPlans([F('1', 'a.docx')], [C('c1', '技术谱系')])
    expect(plans[0].deferred).toBe(true)
  })

  it('待确认的文件不迁移', () => {
    const plans = buildPlans(
      [F('1', 'a.docx', { categoryId: 'c1', needReview: true })],
      [C('c1', '技术谱系')]
    )
    expect(plans[0].deferred).toBe(true)
  })

  it('分类 id 不存在时按未归类处理，不崩', () => {
    const plans = buildPlans([F('1', 'a.docx', { categoryId: '不存在' })], [C('c1', '技术谱系')])
    expect(plans[0].deferred).toBe(true)
  })

  it('归档时优先用建议文件名', () => {
    const plans = buildPlans(
      [F('1', '旧名.docx', { categoryId: 'c1', suggestedName: '技术谱系总览.docx' })],
      [C('c1', '技术谱系')]
    )
    expect(plans[0].targetName).toBe('技术谱系总览.docx')
  })
})

describe('迁移计划：同名永不覆盖', () => {
  it('同一目录下的同名文件自动加序号', () => {
    const files = [F('1', '纪要.docx', { categoryId: 'c1' }), F('2', '纪要.docx', { categoryId: 'c1' })]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans[0].targetName).toBe('纪要.docx')
    expect(plans[0].conflict).toBe(false)
    expect(plans[1].targetName).toBe('纪要_1.docx')
    expect(plans[1].conflict).toBe(true)
  })

  it('三份同名依次排到 _2', () => {
    const files = [1, 2, 3].map((i) => F(String(i), '纪要.docx', { categoryId: 'c1' }))
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans.map((p) => p.targetName)).toEqual(['纪要.docx', '纪要_1.docx', '纪要_2.docx'])
  })

  it('没有扩展名也能加序号', () => {
    const files = [F('1', 'LICENSE', { categoryId: 'c1' }), F('2', 'LICENSE', { categoryId: 'c1' })]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans.map((p) => p.targetName)).toEqual(['LICENSE', 'LICENSE_1'])
  })

  it('不同目录下的同名文件互不影响', () => {
    const files = [
      F('1', '纪要.docx', { categoryId: 'c1' }),
      F('2', '纪要.docx', { categoryId: 'c2' }),
    ]
    const plans = buildPlans(files, [C('c1', '技术谱系'), C('c2', '人工智能推进会')])
    expect(plans.map((p) => p.targetName)).toEqual(['纪要.docx', '纪要.docx'])
    expect(plans.every((p) => !p.conflict)).toBe(true)
  })

  it('不迁移的文件不占名字，后面的文件不必改名', () => {
    const files = [
      F('1', '纪要.docx'),
      F('2', '纪要.docx', { categoryId: 'c1' }),
    ]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans[0].deferred).toBe(true)
    expect(plans[1].targetName).toBe('纪要.docx')
    expect(plans[1].conflict).toBe(false)
  })

  it('类名带斜杠时目录名被清洗，不会建出子目录', () => {
    const plans = buildPlans([F('1', 'a.docx', { categoryId: 'c1' })], [C('c1', 'A/B 方案')])
    expect(plans[0].targetDir).toBe('A_B 方案')
  })
})
