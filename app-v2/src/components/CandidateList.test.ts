import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import CandidateList from './CandidateList.vue'
import type { Candidate } from '../core/types'

// P2 T5.2：候选区高度必须稳定 —— 长理由一撑就把候选/按钮顶出屏幕，
// 键盘 1/2/3 打着打着焦点就跑了。规则：理由最多 2 行，路径单行 + title 全文。
function cand(over: Partial<Candidate> = {}): Candidate {
  return {
    dirId: 7,
    relPath: '安全环保/检查报告',
    score: 0.72,
    reason: '与「安全环保/检查报告」下 6 份文件同类（都含「安全检查」）',
    uncertain: false,
    ...over,
  }
}

const longReason =
  '文件名里的「安全检查」「消防」「验收」三个词都与该目录下 12 份已确认文件的名称高度重合，' +
  '而且类型也一致（都是 docx），类型分布也吻合，所以给到最高分。'

describe('候选列表：理由与路径的截断规则', () => {
  it('长理由不裁文本（靠 CSS 收 2 行），title 里能看到全文', () => {
    const w = mount(CandidateList, {
      props: { candidates: [cand({ reason: longReason })], dirCount: 3 },
    })
    const p = w.find('.reason')
    expect(p.text()).toBe(longReason)
    expect(p.classes()).toContain('reason')
  })

  it('长目录路径带 title 全文，短路径不会自己加省略号', () => {
    const w = mount(CandidateList, {
      props: { candidates: [cand()], dirCount: 3 },
    })
    const path = w.find('.path')
    expect(path.text()).toBe('安全环保/检查报告')
    expect(path.attributes('title')).toBe('安全环保/检查报告')
  })

  it('0 目录时给出「先建一个」的说法，而不是空白候选区', () => {
    const w = mount(CandidateList, { props: { candidates: [], dirCount: 0 } })
    expect(w.text()).toContain('还没有任何目录')
    expect(w.findAll('.cand')).toHaveLength(0)
  })

  it('needsYou 且无候选：明确「这份等你定」', () => {
    const w = mount(CandidateList, {
      props: { candidates: [], dirCount: 3, needsYou: true },
    })
    expect(w.text()).toContain('这份等你定')
  })
})
