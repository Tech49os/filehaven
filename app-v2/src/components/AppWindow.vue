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
    <!-- 侧栏贯通到顶：灯区与 slot 共用同一块 .glass-side，灯区不另加材质/分隔线 -->
    <div v-if="$slots.side" class="side-rail glass-side" data-tauri-drag-region>
      <div class="lights">
        <button
          class="lamp r"
          type="button"
          aria-label="关闭窗口"
          title="关闭"
          @click="onClose"
        ></button>
        <button
          class="lamp y"
          type="button"
          aria-label="最小化窗口"
          title="最小化"
          @click="onMin"
        ></button>
        <button
          class="lamp g"
          type="button"
          aria-label="最大化或还原窗口"
          title="最大化/还原"
          @click="onMax"
        ></button>
      </div>
      <div class="side-body"><slot name="side" /></div>
    </div>

    <div class="content">
      <header class="titlebar glass-title" data-tauri-drag-region>
        <div class="title">{{ title }}</div>
        <div class="tail"><slot name="tail" /></div>
      </header>
      <main class="body">
        <slot />
      </main>
    </div>
  </div>
</template>

<style scoped>
.win {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: row;
  /* 背景由 glass.css 的 .glass-frame 提供（渐变底图供四层材质采样） */
  border-radius: var(--r-window);
  border: 0.5px solid var(--c-border-strong);
  overflow: hidden;
}

/* 整根 rail 从 y=0 贯通到底；材质由 .glass-side 统一提供 */
.side-rail {
  width: var(--side-width);
  flex: 0 0 var(--side-width);
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.lights {
  height: var(--titlebar-h);
  flex: 0 0 var(--titlebar-h);
  display: flex;
  align-items: center;
  padding-left: 22px;
  gap: 8px;
  user-select: none;
}

/* 灯本身是 button，绝不能带 data-tauri-drag-region，否则点不到 */
.lamp {
  width: 12px;
  height: 12px;
  padding: 0;
  margin: 0;
  border: 0;
  border-radius: 50%;
  display: block;
  cursor: pointer;
}
.lamp.r {
  background: #ff5f57;
}
.lamp.y {
  background: #febc2e;
}
.lamp.g {
  background: #28c840;
}
.lamp:hover {
  filter: brightness(0.94);
}
.lamp:focus-visible {
  outline: 2px solid var(--c-primary);
  outline-offset: 2px;
}

.side-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

/* 标题栏只盖内容区；右上角三键已删除，右上角留给 #tail */
.titlebar {
  height: var(--titlebar-h);
  flex: 0 0 var(--titlebar-h);
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: 0 16px;
  position: relative;
  user-select: none;
}

.title {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text-2);
  letter-spacing: 0.1px;
  white-space: nowrap;
}

.tail {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
