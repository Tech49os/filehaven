import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'

const previewDirName = vi.fn()
vi.mock('../core/ipc', () => ({ previewDirName }))
vi.mock('../stores/dirs', () => ({
  useDirsStore: () => ({ allPaths: ['安全检查'], count: 1, load: vi.fn() }),
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

  it('Esc 取消且不拦字母键（输入框里能写 s/p/n）', async () => {
    const w = mount(NewDirInput)
    const input = w.find('input')
    await input.setValue('s')
    expect((input.element as HTMLInputElement).value).toBe('s')
    await input.trigger('keydown', { key: 'Escape' })
    expect(w.emitted('cancel')).toHaveLength(1)
  })
})
