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
  it('同一目录下的同名文件补「副本2」而不是下划线序号', () => {
    const files = [F('1', '纪要.docx', { categoryId: 'c1' }), F('2', '纪要.docx', { categoryId: 'c1', mtime: 999 })]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    // 新的在前，原件保留原名，旧的那份补副本号
    expect(plans[0].targetName).toBe('纪要.docx')
    expect(plans[0].renamed).toBe(false)
    expect(plans[1].targetName).toBe('纪要 (副本2).docx')
    expect(plans[1].renamed).toBe(true)
    // 已经靠副本号分开了，就不算「同名冲突」
    expect(plans.every((p) => !p.conflict)).toBe(true)
  })

  it('三份同名依次排到 副本2 / 副本3', () => {
    const files = [1, 2, 3].map((i) => F(String(i), '纪要.docx', { categoryId: 'c1', mtime: 100 + i }))
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans.map((p) => p.targetName)).toEqual(['纪要 (副本3).docx', '纪要 (副本2).docx', '纪要.docx'])
  })

  it('没有扩展名也能补副本号', () => {
    const files = [F('1', 'LICENSE', { categoryId: 'c1' }), F('2', 'LICENSE', { categoryId: 'c1', mtime: 1 })]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans.map((p) => p.targetName)).toEqual(['LICENSE', 'LICENSE (副本2)'])
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

describe('迁移计划：版本族命名规范化', () => {
  it('同一份文档的多个版本，版本号统一挪到末尾括号', () => {
    const files = [
      F('1', '技术方案V1.docx', { categoryId: 'c1', mtime: 100 }),
      F('2', '技术方案v2.1.docx', { categoryId: 'c1', mtime: 200 }),
    ]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans.map((p) => p.targetName)).toEqual(['技术方案 (v1).docx', '技术方案 (v2.1).docx'])
    expect(plans.every((p) => p.renamed)).toBe(true)
  })

  it('没有同族的单份文件不动名字（哪怕是 V1）', () => {
    const plans = buildPlans([F('1', '技术方案V1.docx', { categoryId: 'c1' })], [C('c1', '技术谱系')])
    expect(plans[0].targetName).toBe('技术方案V1.docx')
    expect(plans[0].renamed).toBe(false)
  })

  it('版本族不跨目录——两个事项目录各有一份「方案V1」互不影响', () => {
    const files = [
      F('1', '方案V1.docx', { categoryId: 'c1' }),
      F('2', '方案V2.docx', { categoryId: 'c2' }),
    ]
    const plans = buildPlans(files, [C('c1', '甲'), C('c2', '乙')])
    expect(plans.map((p) => p.targetName)).toEqual(['方案V1.docx', '方案V2.docx'])
  })

  it('AI 建议名优先，不被版本规范化覆盖', () => {
    const files = [
      F('1', '方案V1.docx', { categoryId: 'c1', suggestedName: '技术方案（改名）.docx' }),
      F('2', '方案V2.docx', { categoryId: 'c1', mtime: 999 }),
    ]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans[0].targetName).toBe('技术方案（改名）.docx')
    expect(plans[0].renamed).toBe(false)
  })

  it('待确认的文件不迁移也不改名', () => {
    const files = [
      F('1', '方案V1.docx', { needReview: true }),
      F('2', '方案V2.docx', { needReview: true }),
    ]
    const plans = buildPlans(files, [C('c1', '技术谱系')])
    expect(plans.map((p) => p.targetName)).toEqual(['方案V1.docx', '方案V2.docx'])
    expect(plans.every((p) => p.deferred)).toBe(true)
  })
})
