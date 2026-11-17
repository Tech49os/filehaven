<script setup lang="ts">
withDefaults(
  defineProps<{ status?: 'idle' | 'running' | 'paused'; text?: string }>(),
  { status: 'idle', text: '' }
)

const DEFAULT_TEXT = {
  idle: '空闲时自动整理 · 已开启',
  running: '后台整理运行中 · 空闲时自动整理',
  paused: '自动整理已暂停',
}
</script>

<template>
  <div class="pill" :class="status">
    <span class="dot" />
    <span class="txt">{{ text || DEFAULT_TEXT[status] }}</span>
  </div>
</template>

<style scoped>
.pill {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 13px;
  border-radius: var(--r-pill);
  background: var(--c-success-bg);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--c-success);
  box-shadow: 0 0 0 3px rgba(40, 200, 64, 0.18);
}
.txt {
  font-size: 12px;
  font-weight: 500;
  color: var(--c-success-deep);
}
.pill.paused {
  background: rgba(142, 142, 147, 0.12);
}
.pill.paused .dot {
  background: var(--c-text-3);
  box-shadow: none;
}
.pill.paused .txt {
  color: var(--c-text-2);
}
.pill.running .dot {
  animation: pulse 1.6s ease-in-out infinite;
}
@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.35;
  }
}
</style>
