<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppWindow from './components/AppWindow.vue'
import Sidebar from './components/Sidebar.vue'
import { useAppStore } from './stores/app'

const app = useAppStore()
const route = useRoute()
const router = useRouter()

const showSidebar = computed(() => route.meta.sidebar !== false)
const title = computed(() => (route.meta.title as string | undefined) ?? '文件管家')

/**
 * 首屏分岔：没配置完 → 引导；配置完了还停在引导 → 整理台。
 * 只做一次，之后由页面自己跳，避免和用户的操作抢方向。
 */
onMounted(async () => {
  await app.refreshBoot()
  if (!app.ready) return
  const onOnboarding = route.path.startsWith('/onboarding')
  if (!app.configured && !onOnboarding) {
    void router.replace('/onboarding')
  } else if (app.configured && onOnboarding) {
    void router.replace('/workspace')
  }
})

// 引导里点「先跳过」后，配置可能仍不完整；这里只保证路径合法，不强制跳
watch(
  () => route.path,
  (p) => {
    if (!app.ready) return
    if (p === '/' ) void router.replace(app.configured ? '/workspace' : '/onboarding')
  },
)
</script>

<template>
  <AppWindow :title="title">
    <div class="shell">
      <Sidebar v-if="showSidebar" />
      <div class="main">
        <p v-if="app.error" class="err" role="alert">{{ app.error }}</p>
        <RouterView v-slot="{ Component }">
          <component :is="Component" />
        </RouterView>
      </div>
    </div>
  </AppWindow>
</template>

<style>
html,
body,
#app {
  height: 100%;
  margin: 0;
  background: transparent;
  font-family: var(--font);
  color: var(--c-text);
  font-size: 13px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}

* {
  box-sizing: border-box;
}

/* 可读性硬约束（验收 9）：正文最小 13px，焦点环统一 Apple 蓝 */
:focus-visible {
  outline: 2px solid var(--c-primary);
  outline-offset: 2px;
}
</style>

<style scoped>
.shell {
  flex: 1;
  min-height: 0;
  display: flex;
}

.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--pad-content);
  gap: var(--gap-grid);
  overflow: hidden;
}

.err {
  margin: 0 0 2px;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-danger-bg);
  color: var(--c-danger);
  font-size: 12px;
}
</style>
