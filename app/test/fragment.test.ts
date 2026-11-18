import { describe, it, expect } from 'vitest'
import { collectCandidates } from '../src/core/emergence/candidates'

const F = (name: string) => ({ name, path: `D:\\文档\\${name}` })

/** 5 份「智能工厂进展汇报」+ 4 份「智能工厂开展汇报」，复现真实数据里的形状 */
const reportFiles = () => [
  ...['甲', '乙', '丙', '丁', '戊'].map((n) => F(`${n}-智能工厂进展汇报.docx`)),
  ...['己', '庚', '辛', '壬'].map((n) => F(`${n}-智能工厂开展汇报.docx`)),
]

describe('跨父词的公共尾巴要被剔掉', () => {
  it('「展汇报」既是进展汇报的尾巴又是开展汇报的尾巴，不该进候选池', () => {
    // 它是长段滑窗的产物：df 9 = 进展汇报 5 + 开展汇报 4，自己从没独立出现过
    const terms = collectCandidates(reportFiles(), { topK: 9999 }).map((c) => c.term)
    expect(terms).not.toContain('展汇报')
  })

  it('两种完整说法都还在，没被碎片连累', () => {
    const terms = collectCandidates(reportFiles(), { topK: 9999 }).map((c) => c.term)
    expect(terms).toContain('智能工厂进展汇报')
    expect(terms).toContain('智能工厂开展汇报')
  })

  it('已知取舍：后缀链上只留最完整的那个，「进展汇报」会被长段吸收', () => {
    // 规则 2 的判据是「左邻唯一且拼上去也是候选」。这里「进展汇报」的左边
    // 永远只有「厂」，而「厂进展汇报」也是同一个长段的滑窗，于是它被判成尾巴。
    // 代价只是类名多了「智能工厂」这个前缀，不影响归属，不值得为它再堆规则
    // （试过更激进的版本：归类从 304 掉到 276）。
    const terms = collectCandidates(reportFiles(), { topK: 9999 }).map((c) => c.term)
    expect(terms).not.toContain('进展汇报')
  })
})

describe('左邻多样的真词不受影响', () => {
  it('「技术谱系」左边可以是各种词，不该被当碎片', () => {
    const files = [
      F('甲-技术谱系梳理.docx'),
      F('乙-技术谱系汇报.docx'),
      F('丙-智能感知技术谱系.docx'),
      F('丁-技术谱系说明.docx'),
      F('戊-软件开发技术谱系.docx'),
    ]
    const terms = collectCandidates(files, { topK: 9999 }).map((c) => c.term)
    expect(terms).toContain('技术谱系')
  })

  it('父词份数之和明显大于自己时不算公共尾巴（「技术室」这类真名）', () => {
    // 6 份「XX技术室」，其中 3 份同属「智能工厂技术室」这一批 →
    // 求和会翻倍，说明「技术室」在别处独立出现过，不是碎片
    const files = [
      ...['甲', '乙', '丙'].map((n) => F(`${n}-智能工厂技术室.docx`)),
      F('丁-技术室成本反馈.docx'),
      F('戊-各部门技术室名单.docx'),
      F('己-技术室职责.docx'),
    ]
    const terms = collectCandidates(files, { topK: 9999 }).map((c) => c.term)
    expect(terms).toContain('技术室')
  })
})
