import { describe, expect, it } from 'vitest'
import {
  buildFamilies,
  duplicateGroups,
  familyRenames,
  latestOnly,
  parseName,
  type FamFile,
} from '../src/core/version'

const F = (id: string, name: string, mtime = 1000, size = 100): FamFile => ({
  id,
  name,
  size,
  mtime,
})

describe('版本标记解析', () => {
  it('末尾 V1.1 剥出来，主名保留原文', () => {
    const p = parseName('中海油智能工厂梯度培育培训项目项目方案V1.1.docx')
    expect(p.title).toBe('中海油智能工厂梯度培育培训项目项目方案')
    expect(p.marks).toEqual([{ kind: 'ver', label: 'v1.1', order: 1.1 }])
  })

  it('V1 和 v1.0 归一化到同一个主名（大小写、点号不影响）', () => {
    expect(parseName('方案V1.docx').key).toBe(parseName('方案v1.0.docx').key)
    expect(parseName('方案V1.docx').key).toBe(parseName('方案.docx').key)
  })

  it('Windows 重复下载的 (1) 当成副本，序号是 2（原件算第 1 份）', () => {
    const p = parseName('AI智能编程工具技术进展与对标分析汇报 (1).pptx')
    expect(p.title).toBe('AI智能编程工具技术进展与对标分析汇报')
    expect(p.marks).toEqual([{ kind: 'copy', label: '副本2', order: 2 }])
  })

  it('前缀的「副本」当噪音剥掉，只留 (1) 这条痕迹', () => {
    const p = parseName('副本AI应用落地 - 赵阳彦威补充(1).xlsx')
    expect(p.title).toBe('AI应用落地 - 赵阳彦威补充')
    expect(p.marks.map((m) => m.label)).toEqual(['副本2'])
  })

  it('「修改/修订」当前面没有分隔符时是动词，不能当版本痕迹', () => {
    expect(parseName('ppt修改.pptx').title).toBe('ppt修改')
    expect(parseName('ppt_修改.pptx').title).toBe('ppt')
  })

  it('末尾日期参与「是不是同一份」的判断，但不挪位置、不改名', () => {
    const a = parseName('报价单-20240315.xlsx')
    const b = parseName('报价单-20240420.xlsx')
    expect(a.key).toBe(b.key)
    // 日期是名字的一部分，原样留在原位
    expect(a.title).toBe('报价单-20240315')
    expect(a.marks).toEqual([])

    // 「2024年度总结」和「2023年度总结」是两份不同的东西，绝不能并成一族
    expect(parseName('2024年度总结.docx').key).not.toBe(parseName('2023年度总结.docx').key)
    expect(parseName('2024年度总结.docx').title).toBe('2024年度总结')
  })

  it('日期在中间夹着版本号时保守处理：认得出自家人，但不擅自改名', () => {
    const p = parseName('人工智能推进会-生产智能化项目组-2026.8.4(1).pptx')
    expect(p.key).toBe(parseName('人工智能推进会-生产智能化项目组-2026.8.4.pptx').key)
    // 只剥 (1)，会议日期必须留在名字里
    expect(p.title).toBe('人工智能推进会-生产智能化项目组-2026.8.4')
  })

  it('中文版次 / 终版 / 草稿都认', () => {
    expect(parseName('技术方案第3版.docx').marks[0]).toMatchObject({ kind: 'ver', label: '第3版' })
    expect(parseName('技术方案_终版.docx').marks[0]).toMatchObject({ kind: 'ver', label: '终版' })
    expect(parseName('04生产工艺设备服务器等硬件设备支撑（二期）_初版草稿_完.docx').marks.map((m) => m.label)).toEqual([
      '完',
      '草稿',
      '初版',
    ])
  })

  it('括号里是中文的（信科天分）不是副本标记', () => {
    const p = parseName('附件2：服务器领用交接单 -（信科天分）.docx')
    expect(p.title).toBe('附件2：服务器领用交接单 -（信科天分）')
    expect(p.marks).toEqual([])
  })

  it('剥完什么都不剩的名字不成族（(1).docx 这种）', () => {
    expect(parseName('(1).docx').key).toBe('')
  })
})

describe('归族', () => {
  it('同一份材料的五个版本归成一族，最新在前', () => {
    const fams = buildFamilies([
      F('a', '附件2：服务器领用交接单 -（信科天分）.docx', 1000),
      F('b', '附件2：服务器领用交接单 -（信科天分）(1).docx', 1200),
      F('c', '附件2：服务器领用交接单 -（信科天分）(2).docx', 900),
      F('d', '附件2：服务器领用交接单 -（信科天分）v1.0.docx', 1500),
      F('e', '附件2：服务器领用交接单 -（信科天分）.pdf', 800),
    ])
    expect(fams).toHaveLength(1)
    expect(fams[0].files.map((f) => f.id)).toEqual(['d', 'b', 'a', 'c', 'e'])
    expect(fams[0].latestId).toBe('d')
    expect(fams[0].title).toBe('附件2：服务器领用交接单 -（信科天分）')
  })

  it('只有一份的不是族（界面上不该出现折叠按钮）', () => {
    expect(buildFamilies([F('a', '独一份.docx'), F('b', '另一份.docx')])).toEqual([])
  })

  it('名字一样但内容不同也算一族——它们本来就是同一份文档的两个副本', () => {
    const fams = buildFamilies([F('a', 'ppt修改.pptx', 1000, 645324), F('b', 'ppt修改.pptx', 1200, 506554)])
    expect(fams[0].files.map((f) => f.id)).toEqual(['b', 'a'])
  })

  it('跨年份的同名标题不并族', () => {
    expect(buildFamilies([F('a', '2023年度总结.docx'), F('b', '2024年度总结.docx')])).toEqual([])
  })

  it('不同扩展名算同族（pdf 常是盖章导出件）', () => {
    const fams = buildFamilies([F('a', '技术方案.docx'), F('b', '技术方案.pdf')])
    expect(fams).toHaveLength(1)
  })

  it('同主名的 html/js/css 不是一份文档的三种格式，不并族', () => {
    expect(
      buildFamilies([
        F('a', 'work-item-detail.html'),
        F('b', 'work-item-detail.js'),
        F('c', 'work-item-detail.css'),
      ])
    ).toEqual([])
  })

  it('png 和 drawio 也不并族（一个是导出图、一个是源文件，但名字只是同名）', () => {
    expect(buildFamilies([F('a', '部署架构图.png'), F('b', '部署架构图.drawio')])).toEqual([])
  })

  it('表格和文档不同类，不并族', () => {
    expect(buildFamilies([F('a', '清单.docx'), F('b', '清单.xlsx')])).toEqual([])
  })

  it('空名字/纯标记不参与成族', () => {
    expect(buildFamilies([F('a', '(1).docx'), F('b', '(2).docx')])).toEqual([])
  })
})

describe('规范化命名', () => {
  const rename = (files: FamFile[]) => {
    const m = familyRenames(files)
    return files.map((f) => m.get(f.id) ?? f.name).sort()
  }

  it('版本号统一成末尾括号，最新那份不带标记的保留原名', () => {
    const files = [
      F('a', '技术方案.docx', 1000),
      F('b', '技术方案V1.docx', 900),
      F('c', '技术方案v2.1.docx', 1100),
    ]
    expect(rename(files)).toEqual(['技术方案 (v1).docx', '技术方案 (v2.1).docx', '技术方案.docx'])
  })

  it('两个同名副本要分出「副本2」，原件不动', () => {
    const files = [F('a', 'ppt修改.pptx', 1000), F('b', 'ppt修改.pptx', 1200)]
    expect(rename(files)).toEqual(['ppt修改 (副本2).pptx', 'ppt修改.pptx'])
  })

  it('(1)(2) 连号排到副本2 / 副本3', () => {
    const files = [
      F('a', '汇报.pptx', 1000),
      F('b', '汇报(1).pptx', 900),
      F('c', '汇报(2).pptx', 800),
    ]
    expect(rename(files)).toEqual(['汇报 (副本2).pptx', '汇报 (副本3).pptx', '汇报.pptx'])
  })

  it('扩展名不同就不加后缀——它们本来不会撞名', () => {
    const files = [F('a', '技术方案.docx', 1000), F('b', '技术方案.pdf', 900)]
    expect(rename(files)).toEqual(['技术方案.docx', '技术方案.pdf'])
  })

  it('改名结果绝不重复', () => {
    const files = [
      F('a', '方案V1.docx', 1000),
      F('b', '方案v1.docx', 1100),
      F('c', '方案V1(1).docx', 1200),
      F('d', '方案.xlsx', 1300),
    ]
    const out = rename(files)
    expect(new Set(out).size).toBe(out.length)
  })

  it('不成族的单份文件不该被改名', () => {
    expect(familyRenames([F('a', '技术方案V1.docx')]).size).toBe(0)
  })
})

describe('统计去重：一个版本族只算一份', () => {
  it('只留最新版参与候选统计，其他版本不消失只是不计票', () => {
    const files = [
      F('a', '集团汇报V1.docx', 100),
      F('b', '集团汇报V2.docx', 200),
      F('c', '集团汇报V3.docx', 300),
      F('d', '另一份文件.docx', 50),
    ]
    expect(latestOnly(files).map((f) => f.id)).toEqual(['c', 'd'])
  })

  it('没有版本族时原样返回（不复制、不重排）', () => {
    const files = [F('a', '甲.docx'), F('b', '乙.docx')]
    expect(latestOnly(files)).toBe(files)
  })
})

describe('内容完全相同的副本', () => {
  it('同哈希的互相认得，三份就互相列另外两份', () => {
    const d = duplicateGroups([
      { id: 'a', hash: 'x' },
      { id: 'b', hash: 'x' },
      { id: 'c', hash: 'y' },
      { id: 'd', hash: 'x' },
    ])
    expect(d.get('a')!.map((f) => f.id)).toEqual(['b', 'd'])
    expect(d.has('c')).toBe(false)
  })

  it('没算哈希的文件不参与（不能凭大小猜）', () => {
    expect(duplicateGroups([{ id: 'a' }, { id: 'b' }]).size).toBe(0)
  })

  it('空哈希不当成一份（否则所有没算的文件会被算成同一个）', () => {
    expect(duplicateGroups([{ id: 'a', hash: '' }, { id: 'b', hash: '' }]).size).toBe(0)
  })
})
