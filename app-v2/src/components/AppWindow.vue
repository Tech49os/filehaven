<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'

defineProps<{ title?: string }>()

// 纯 vite dev（浏览器里跑）时没有 Tauri 上下文，按钮点了不报错即可
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
const win = isTauri ? getCurrentWindow() : null

function onMin() {
  void win?.minimize()
}
function onMax() {
  void win?.toggleMaximize()
}
function onClose() {
  void win?.close()
}
</script>

<template>
  <div class="win glass-frame">
    <header class="titlebar glass-title" data-tauri-drag-region>
      <div class="lights" aria-hidden="true">
        <i class="r" /><i class="y" /><i class="g" />
      </div>
      <div class="title" data-tauri-drag-region>{{ title }}</div>
      <div class="tail"><slot name="tail" /></div>
      <div class="winbtns">
        <button class="wb" title="最小化" @click="onMin">&#8211;</button>
        <button class="wb" title="最大化" @click="onMax">&#9633;</button>
        <button class="wb close" title="关闭" @click="onClose">&#10005;</button>
      </div>
    </header>
    <main class="body">
      <slot />
    </main>
  </div>
</template>

<style scoped>
.win {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  /* 背景由 glass.css 的 .glass-frame 提供（渐变底图供四层材质采样） */
  border-radius: var(--r-window);
  border: 0.5px solid var(--c-border-strong);
  overflow: hidden;
}

.titlebar {
  height: 44px;
  flex: 0 0 44px;
  display: flex;
  align-items: center;
  padding: 0 8px 0 16px;
  /* 背景与分隔线交给 glass.css 的 .glass-title（半透明 + blur + saturate） */
  position: relative;
  user-select: none;
}

.lights {
  display: flex;
  gap: 8px;
  width: 76px;
}
.lights i {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  display: block;
}
.lights .r {
  background: #ff5f57;
}
.lights .y {
  background: #febc2e;
}
.lights .g {
  background: #28c840;
}

.title {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text-2);
  letter-spacing: 0.1px;
}

.tail {
  margin-left: 76px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.winbtns {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 2px;
}

.wb {
  width: 34px;
  height: 26px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--c-text-2);
  font-size: 12px;
  line-height: 1;
  cursor: default;
}
.wb:hover {
  background: var(--c-hover);
}
.wb.close:hover {
  background: var(--c-danger);
  color: #fff;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
