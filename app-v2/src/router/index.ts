import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'

/**
 * hash 路由：Tauri 打包后是 `tauri://localhost`，
 * history 模式在刷新/deep link 时会 404，hash 最稳。
 *
 * `meta.sidebar === false` 的页面自己占满窗口（引导流程不显示左侧栏）。
 */
const routes: RouteRecordRaw[] = [
  { path: '/', redirect: '/workspace' },
  {
    path: '/onboarding',
    name: 'onboarding',
    component: () => import('../pages/onboarding/Onboarding.vue'),
    meta: { sidebar: false, title: '开始使用' },
  },
  {
    path: '/onboarding/dirs',
    name: 'dirSuggest',
    component: () => import('../pages/onboarding/DirSuggest.vue'),
    meta: { sidebar: false, title: '目录方案' },
  },
  {
    path: '/workspace',
    name: 'workspace',
    component: () => import('../pages/workspace/Workspace.vue'),
    meta: { title: '整理台' },
  },
  {
    path: '/migration',
    name: 'migration',
    component: () => import('../pages/migration/PendingMigration.vue'),
    meta: { title: '待迁移' },
  },
  {
    path: '/browser',
    name: 'browser',
    component: () => import('../pages/browser/Browser.vue'),
    meta: { title: '浏览' },
  },
  {
    path: '/parked',
    name: 'parked',
    component: () => import('../pages/parked/Parked.vue'),
    meta: { title: '待定区' },
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('../pages/settings/Settings.vue'),
    meta: { title: '设置' },
  },
  { path: '/:pathMatch(.*)*', redirect: '/workspace' },
]

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

export default router
