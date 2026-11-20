import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

const suggestDirs = vi.fn()
const acceptDirs = vi.fn()
const createDir = vi.fn()
const previewDirName = vi.fn()
const replace = vi.fn()

vi.mock('../../core/ipc', () => ({
  suggestDirs,
  acceptDirs,
  createDir,
  previewDirName,
  listDirs: vi.fn(),
}))
vi.mock('../../core/events', () => ({ onSuggestStage: async () => () => {} }))
vi.mock('vue-router', () => ({ useRouter: () => ({ replace, push: replace }) }))
vi.mock('../../stores/dirs', () => ({
  useDirsStore: () => ({ count: 1, load: vi.fn(), allPaths: [] }),
}))

const DirSuggest = (await import('./DirSuggest.vue')).default

const TWO_OPTIONS = {
  optionA: [
    { name: '安全检查', basis: '8 份文件名里都有「安全检查」', fileCount: 8 },
    { name: '合同', basis: '5 份文件名含「合同/采购」', fileCount: 5 },
  ],
  optionB: [
    { name: '安全/检查记录', basis: '按业务对象 + 文档类型拆', fileCount: 6 },
  ],
  dimensionA: '按事项',
  dimensionB: '按文档用途',
  model: 'qwen2.5:7b',
  elapsedMs: 42000,
}

describe('目录方案页（T6.7）', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    suggestDirs.mockReset()
    acceptDirs.mockReset().mockResolvedValue([])
    replace.mockReset()
  })

  it('正常：双列并排，能展开看依据，勾选后采纳传的是目录名', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    const w = mount(DirSuggest)
    await flushPromises()

    expect(w.text()).toContain('方案 A')
    expect(w.text()).toContain('方案 B')
    // 两套的区别就在维度上，列标题必须带上，否则用户看到的是两个「方案」而不知道差在哪
    expect(w.text()).toContain('按事项')
    expect(w.text()).toContain('按文档用途')
    expect(w.text()).toContain('安全检查')

    // 依据默认收起，点开才显示
    expect(w.text()).not.toContain('8 份文件名里都有')
    await w.findAll('.basis-btn')[0].trigger('click')
    expect(w.text()).toContain('8 份文件名里都有')

    await w.findAll('input[type="checkbox"]')[0].setValue(true)
    await w.findAll('input[type="checkbox"]')[1].setValue(true)
    const acceptBtn = w.findAll('button').find((b) => b.text().includes('采纳并建目录'))
    expect(acceptBtn!.text()).toContain('2')
    await acceptBtn!.trigger('click')
    await flushPromises()

    expect(acceptDirs).toHaveBeenCalledWith(['安全检查', '合同'])
    expect(replace).toHaveBeenCalledWith('/workspace')
  })

  it('换一批：force=true 重新请求', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    const w = mount(DirSuggest)
    await flushPromises()
    await w.findAll('button').find((b) => b.text() === '换一批')!.trigger('click')
    await flushPromises()
    expect(suggestDirs).toHaveBeenLastCalledWith(true, '')
  })

  it('模型不可用：整块降级成手动输入，并说清原因', async () => {
    suggestDirs.mockRejectedValue({ code: 'MODEL_DOWN', message: '本地模型不可达' })
    const w = mount(DirSuggest)
    await flushPromises()

    expect(w.text()).toContain('本地模型不可达')
    expect(w.find('.degrade').exists()).toBe(true)
    // 降级后直接给输入框，不让用户卡住
    expect(w.find('input.input').exists()).toBe(true)
    expect(w.text()).toContain('自己写')
  })

  it('方案为空也算降级，不显示空白双列', async () => {
    suggestDirs.mockResolvedValue({ optionA: [], optionB: [], model: 'x', elapsedMs: 1 })
    const w = mount(DirSuggest)
    await flushPromises()
    expect(w.find('.cols').exists()).toBe(false)
    expect(w.text()).toContain('模型这次没给出可用方案')
  })

  // ---------------------------------------------------------------- 补充要求

  // 反馈原话：「无法将我的主观意见给到模型，只能让模型一遍又一遍的生成没什么意义」
  it('写下自己的要求，能带着它让模型重来一版', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    const w = mount(DirSuggest)
    await flushPromises()

    expect(w.find('textarea.hint').exists()).toBe(true)
    await w.find('textarea.hint').setValue('  我们按客户分，主要做招投标  ')
    await w.findAll('button').find((b) => b.text().includes('按我说的'))!.trigger('click')
    await flushPromises()

    // 首尾空白要去掉：后端虽然也会 trim，但界面不该把空格当意见
    expect(suggestDirs).toHaveBeenLastCalledWith(false, '我们按客户分，主要做招投标')
  })

  // 有要求时「换一批」也要带着要求，否则一按就退回老结果
  it('换一批同样带上补充要求', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    const w = mount(DirSuggest)
    await flushPromises()
    await w.find('textarea.hint').setValue('按客户分')
    await w.findAll('button').find((b) => b.text() === '换一批')!.trigger('click')
    await flushPromises()
    expect(suggestDirs).toHaveBeenLastCalledWith(true, '按客户分')
  })

  // ---------------------------------------------------------------- 就地改

  it('方案里能直接改名，采纳时传的是改过的名字', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    previewDirName.mockResolvedValue({ cleaned: '安全检查记录', valid: true, reason: null })
    const w = mount(DirSuggest)
    await flushPromises()

    await w.findAll('.dname').find((n) => n.text() === '安全检查')!.trigger('click')
    const input = w.find('.dname-input')
    expect(input.exists()).toBe(true)
    await input.setValue('安全检查记录')
    await input.trigger('keydown.enter')
    await flushPromises()
    expect(w.text()).toContain('安全检查记录')

    await w.findAll('input[type="checkbox"]')[0].setValue(true)
    await w.findAll('button').find((b) => b.text().includes('采纳并建目录'))!.trigger('click')
    await flushPromises()
    expect(acceptDirs).toHaveBeenCalledWith(['安全检查记录'])
  })

  it('能删掉不想要的目录，也能自己补一条', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    previewDirName.mockResolvedValue({ cleaned: '人事培训', valid: true, reason: null })
    const w = mount(DirSuggest)
    await flushPromises()

    // 删掉「合同」
    const row = w.findAll('.drow').find((r) => r.text().includes('合同'))!
    await row.find('button.del').trigger('click')
    expect(w.findAll('.drow').some((r) => r.text().includes('合同'))).toBe(false)

    // 自己加一条
    await w.findAll('button').find((b) => b.text().includes('再加一条'))!.trigger('click')
    const addInput = w.find('.dname-input')
    await addInput.setValue('人事培训')
    await addInput.trigger('keydown.enter')
    await flushPromises()

    await w.findAll('input[type="checkbox"]')[0].setValue(true)
    await w.findAll('input[type="checkbox"]')[1].setValue(true)
    await w.findAll('button').find((b) => b.text().includes('采纳并建目录'))!.trigger('click')
    await flushPromises()
    expect(acceptDirs).toHaveBeenCalledWith(['安全检查', '人事培训'])
  })

  // 名字里有非法字符时当场说清，别等采纳时才整批失败
  it('改出非法目录名就地提示，且不让提交', async () => {
    suggestDirs.mockResolvedValue(TWO_OPTIONS)
    previewDirName.mockResolvedValue({
      cleaned: '',
      valid: false,
      reason: '目录名不能包含 \\ / : * ? " < > |',
    })
    const w = mount(DirSuggest)
    await flushPromises()

    await w.findAll('.dname').find((n) => n.text() === '合同')!.trigger('click')
    const input = w.find('.dname-input')
    await input.setValue('合同/采购')
    await input.trigger('keydown.enter')
    await flushPromises()

    expect(w.text()).toContain('不能包含')
    await w.findAll('input[type="checkbox"]')[0].setValue(true)
    await w.findAll('button').find((b) => b.text().includes('采纳并建目录'))!.trigger('click')
    await flushPromises()
    expect(acceptDirs).not.toHaveBeenCalled()
  })
})
