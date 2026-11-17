import { createRouter, createWebHashHistory } from 'vue-router'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/wizard' },
    {
      path: '/wizard',
      name: 'wizard',
      component: () => import('../pages/wizard/WizardPage.vue'),
    },
    {
      path: '/main',
      component: () => import('../pages/main/MainLayout.vue'),
      children: [
        { path: '', redirect: '/main/dashboard' },
        {
          path: 'dashboard',
          name: 'dashboard',
          component: () => import('../pages/main/Dashboard.vue'),
        },
        {
          path: 'browse',
          name: 'browse',
          component: () => import('../pages/main/Browse.vue'),
        },
        {
          path: 'pending',
          name: 'pending',
          component: () => import('../pages/main/Pending.vue'),
        },
        {
          path: 'logs',
          name: 'logs',
          component: () => import('../pages/main/Logs.vue'),
        },
        {
          path: 'settings',
          name: 'settings',
          component: () => import('../pages/main/Settings.vue'),
        },
      ],
    },
  ],
})

export default router
