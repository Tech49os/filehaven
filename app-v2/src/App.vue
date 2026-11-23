<script setup lang="ts">
import { computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppWindow from './components/AppWindow.vue'
import Sidebar from './components/Sidebar.vue'
import SearchPanel from './components/SearchPanel.vue'
import { useAppStore } from './stores/app'
import { useUiStore } from './stores/ui'

const app = useAppStore()
const ui = useUiStore()
const route = useRoute()
const router = useRouter()

// 材质开关：默认开（页面不带 no-glass 类），关闭时整页回退实色。
// 挂在 html 上是因为命令面板 Teleport 到 body，不在 .win 子树内。
watch(
  () => ui.glassEnabled,
  (on) => document.documentElement.classList.toggle('no-glass', !on),
  { immediate: true },
)

const showSidebar = computed(() => route.meta.sidebar !== false)
const title = computed(() => (route.meta.title as string | undefined) ?? '文件管家')

/**
 * 右上角「快速检索」胶囊：复用 SearchPanel 已经挂好的全局 Ctrl+K 监听。
 * 派发真实键盘事件比给 SearchPanel 加 defineExpose/ref 更稳，也不用动块②的文件。
 */
function openQuickSearch() {
  window.dispatchEvent(
    new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }),
  )
}

/**
 * 首屏分岔：没配置完 → 引导；配置完了还停在引导 → 整理台。
 * 只做一次，之后由页面自己跳，避免和用户的操作抢方向。
 */
onMounted(async () => {
  // 启动首调才探模型（之后每次判定后的刷新都不探，见 stores/app.ts）
  await app.refreshBoot({ probe: true })
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
    <!-- 引导页 meta.sidebar=false 时不提供 slot，AppWindow 用 $slots.side 判断不渲染 rail -->
    <template v-if="showSidebar" #side>
      <Sidebar />
    </template>
    <template #tail>
      <button
        class="quick-search"
        type="button"
        aria-label="快速检索"
        title="快速检索 Ctrl K"
        @click="openQuickSearch"
      >
        <span>快速检索</span><kbd>Ctrl K</kbd>
      </button>
    </template>
    <div class="main glass-panel">
      <p v-if="app.error" class="err" role="alert">{{ app.error }}</p>
      <RouterView v-slot="{ Component }">
        <Transition name="page" mode="out-in">
          <component :is="Component" :key="route.path" />
        </Transition>
      </RouterView>
    </div>
    <!-- Ctrl+K 命令面板：全局一个，挂在这里（T3.3） -->
    <SearchPanel />
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

/* ------------------------------------------------------------
   动效基线（刻意很轻：只做「东西从哪来」，不做表演）
   ------------------------------------------------------------ */

/* 页面切换：淡入 + 4px 上浮。离开比进入快，操作才跟手 */
.page-enter-active {
  transition: opacity var(--dur-page) var(--ease-out),
    transform var(--dur-page) var(--ease-out);
}
.page-leave-active {
  transition: opacity var(--dur-fast) linear, transform var(--dur-fast) linear;
}
.page-enter-from {
  opacity: 0;
  transform: translateY(6px);
}
.page-leave-to {
  opacity: 0;
  transform: translateY(-3px);
}

/* 卡片换代：换一份文件时不是「啪」地跳变，而是原地淡换 */
.card-enter-active {
  transition: opacity var(--dur-page) var(--ease-out),
    transform var(--dur-page) var(--ease-out);
}
.card-leave-active {
  transition: opacity 70ms linear;
}
.card-enter-from {
  opacity: 0;
  transform: translateY(5px) scale(0.995);
}
.card-leave-to {
  opacity: 0;
}

/* 列表项出现：一行一行浮上来（很短，够看出「多了东西」） */
.rise-enter-active {
  transition: opacity var(--dur) var(--ease-out), transform var(--dur) var(--ease-out);
}
.rise-enter-from {
  opacity: 0;
  transform: translateY(4px);
}

/* 按压反馈：所有按钮点到就有反应，这是「不卡」的主要来源 */
button:not(:disabled) {
  transition: background var(--dur-fast) ease, color var(--dur-fast) ease,
    transform var(--dur-fast) var(--ease-out), opacity var(--dur-fast) ease;
}
button:not(:disabled):active {
  transform: scale(0.97);
}

/* 系统和用户说了「别动」，就真的别动 */
@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    transition-duration: 1ms !important;
    animation-duration: 1ms !important;
    animation-iteration-count: 1 !important;
  }
  button:not(:disabled):active {
    transform: none;
  }
}
</style>

<style scoped>
/* 右上角快速检索胶囊：本块只要可见可点，激活态蓝描边交给块② */
.quick-search {
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 242px;
  max-width: 100%;
  height: 30px;
  padding: 0 10px;
  border: 0.5px solid rgba(255, 255, 255, 0.64);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.56);
  color: var(--c-text-2);
  font-size: 12px;
  font-family: var(--font);
  cursor: pointer;
}
.quick-search kbd {
  font-family: var(--font);
  font-size: 11px;
  color: var(--c-text-3);
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
  /* 右下圆角：.main 挂 .glass-panel，带 backdrop-filter 会独立合成，
     祖先 .win 的 overflow:hidden 裁不掉它，必须自己圆角（同 .side-rail） */
  border-bottom-right-radius: var(--r-window);
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
