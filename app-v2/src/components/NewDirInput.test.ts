import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const previewDirName = vi.fn()
const DIRS = ['安全检查', '安全检查/年度检查', '信息科/生产智能化项目组/汇报']
vi.mock('../core/ipc', () => ({ previewDirName }))
vi.mock('../stores/dirs', () => ({
  useDirsStore: () => ({ allPaths: DIRS, count: DIRS.length, load: vi.fn() }),
}))

const NewDirInput = (await import('./NewDirInput.vue')).default

/** 把 300ms 防抖走完，再等一次微任务队列 */
async function flush(wrapper: ReturnType<typeof mount>) {
  await vi.advanceTimersByTimeAsync(320)
  await nextTick()
  await wrapper.vm.$nextTick()
}

describe('NewDirInput（T6.11）', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    previewDirName.mockReset()
  })
  afterEach(() => vi.useRealTimers())

  it('显示后端清洗结果与最终相对路径', async () => {
    previewDirName.mockResolvedValue({
      cleaned: '安全/检查_报告',
      valid: true,
      reason: null,
    })
    const w = mount(NewDirInput)
    await w.find('input').setValue('安全/检查:报告')
    await flush(w)

    expect(previewDirName).toHaveBeenCalledTimes(1)
    expect(w.text()).toContain('安全/检查_报告')
    expect(w.find('button.primary').attributes('disabled')).toBeUndefined()

    await w.find('button.primary').trigger('click')
    expect(w.emitted('confirm')?.[0]).toEqual(['安全/检查_报告'])
  })

  it('清洗后为空时禁用确认并说明原因', async () => {
    previewDirName.mockResolvedValue({ cleaned: '', valid: false, reason: '清洗后为空' })
    const w = mount(NewDirInput)
    await w.find('input').setValue('...')
    await flush(w)

    expect(w.text()).toContain('清洗后为空')
    expect(w.find('button.primary').attributes('disabled')).toBeDefined()
    await w.find('button.primary').trigger('click')
    expect(w.emitted('confirm')).toBeUndefined()
  })

  it('防抖：连打三次只请求最后一次', async () => {
    previewDirName.mockResolvedValue({ cleaned: 'x', valid: true, reason: null })
    const w = mount(NewDirInput)
    const input = w.find('input')
    await input.setValue('安')
    await input.setValue('安全')
    await input.setValue('安全检查')
    await flush(w)
    expect(previewDirName).toHaveBeenCalledTimes(1)
    expect(previewDirName).toHaveBeenCalledWith('安全检查')
  })

  // 2026-11-20 反馈：「在 N 输入框中要有联想能力，能够快速索引到」。
  // 空输入框不主动弹下拉（太吵），聚焦才列出来。
  it('聚焦后才列已有目录，敲字按 relPath 过滤', async () => {
    const w = mount(NewDirInput)
    expect(w.find('.drop').exists()).toBe(false)

    await w.find('input').trigger('focus')
    expect(w.findAll('.opt')).toHaveLength(3)

    await w.find('input').setValue('年度')
    const opts = w.findAll('.opt')
    expect(opts).toHaveLength(1)
    expect(opts[0].text()).toContain('安全检查/年度检查')
    // 前缀命中排前面：敲「安全」时「安全检查」不该排在深层路径后面
    await w.find('input').setValue('安全')
    expect(w.findAll('.opt')[0].text()).toContain('安全检查')
  })

  it('↓ 选中已有目录：填进输入框，并明说「归到这里」而不是新建', async () => {
    previewDirName.mockResolvedValue({ cleaned: '安全检查', valid: true, reason: null })
    const w = mount(NewDirInput)
    const input = w.find('input')
    await input.trigger('focus')
    await input.trigger('keydown', { key: 'ArrowDown' })
    await input.trigger('keydown', { key: 'ArrowDown' })
    await input.trigger('keydown', { key: 'Enter' })
    await flush(w)

    expect((input.element as HTMLInputElement).value).toBe('安全检查/年度检查')
    expect(w.find('button.primary').text()).toBe('归到这里')
    expect(w.text()).toContain('已有目录')

    await w.find('button.primary').trigger('click')
    expect(w.emitted('confirm')?.[0]).toEqual(['安全检查'])
  })

  it('Esc 先收下拉，再 Esc 才是取消', async () => {
    const w = mount(NewDirInput)
    const input = w.find('input')
    await input.setValue('s')
    expect((input.element as HTMLInputElement).value).toBe('s')

    await input.trigger('keydown', { key: 'Escape' })
    expect(w.find('.drop').exists()).toBe(false)
    expect(w.emitted('cancel')).toBeUndefined()

    await input.trigger('keydown', { key: 'Escape' })
    expect(w.emitted('cancel')).toHaveLength(1)
  })
})
