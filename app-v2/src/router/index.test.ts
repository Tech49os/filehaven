import { describe, expect, it } from 'vitest'
import router from './index'

/** T6.2：七条路由都能解析出组件，不许白屏 */
describe('路由表', () => {
  const paths = [
    '/onboarding',
    '/onboarding/dirs',
    '/workspace',
    '/migration',
    '/browser',
    '/parked',
    '/settings',
  ]

  it('每条路径都能匹配到具名路由', async () => {
    for (const p of paths) {
      await router.push(p)
      expect(router.currentRoute.value.matched.length, p).toBeGreaterThan(0)
      expect(router.currentRoute.value.name, p).toBeTruthy()
    }
  })

  it('/ 重定向到整理台', async () => {
    await router.push('/')
    expect(router.currentRoute.value.path).toBe('/workspace')
  })

  it('未知路径不白屏，回落整理台', async () => {
    await router.push('/不存在的页面')
    expect(router.currentRoute.value.path).toBe('/workspace')
  })

  it('引导页不显示侧边栏（meta.sidebar=false）', async () => {
    await router.push('/onboarding')
    expect(router.currentRoute.value.meta.sidebar).toBe(false)
    await router.push('/workspace')
    expect(router.currentRoute.value.meta.sidebar).not.toBe(false)
  })
})
