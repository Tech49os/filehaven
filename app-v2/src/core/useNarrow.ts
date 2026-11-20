/**
 * 窗口窄不窄（决定整理台是并排三栏还是两栏 + 抽屉）
 *
 * 为什么不用纯 CSS 媒体查询：折叠后要换的是**结构**（aside 变抽屉、顶部多一个开合按钮），
 * 不是几个属性；用 JS 判一次，模板里只有一条真相，也不用写两套 DOM。
 *
 * 1240px 这个阈值是算出来的：默认窗口 1160 宽时，侧栏 200 + 内容边距 48 + 两栏各 208 + 间隙
 * 之后，中间卡片只剩约 468px —— 连一行文件名都放不下。1240 以上才谈得上「三栏并排」。
 */
import { onMounted, onUnmounted, ref } from 'vue'

export const NARROW_MAX = 1240

export function useNarrow(max = NARROW_MAX) {
  const narrow = ref(false)
  let mq: MediaQueryList | null = null

  const apply = (e: { matches: boolean }) => {
    narrow.value = e.matches
  }

  onMounted(() => {
    // 纯浏览器 / 测试环境没有 matchMedia，就老老实实当宽窗
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return
    mq = window.matchMedia(`(max-width: ${max}px)`)
    narrow.value = mq.matches
    mq.addEventListener('change', apply)
  })

  onUnmounted(() => {
    mq?.removeEventListener('change', apply)
    mq = null
  })

  return narrow
}
