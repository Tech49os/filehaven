<script setup lang="ts">
/**
 * Ctrl+K 命令面板（P3 T3.3）。
 *
 * 键位冲突是这页最大的坑（计划点名）：整理台把 Enter/1/2/3/S/P/N 全占了。
 * 面板**不用** keys.ts 的 KeyCtx —— 自己 stopPropagation + panelOpen 开关，
 * 打开时在 capture 阶段把普通按键吞掉，绝不让「在搜索框里敲 p」把文件搁置。
 * 输入 80ms 防抖 → searchFiles({q, scope:'all', limit:20})；↑↓ 选、Enter 打开、Esc 关。
 */
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from './AppIcon.vue'
import { iconOf, kindLabel } from '../core/format'
import { livePath } from '../core/types'
import type { FileRow } from '../core/types'
import * as ipc from '../core/ipc'

const router = useRouter()

const open = ref(false)
const q = ref('')
const rows = ref<FileRow[]>([])
const searched = ref(false)
const cursor = ref(0)
const inputEl = ref<HTMLInputElement | null>(null)
const loading = ref(false)

const dirOf = (r: FileRow) => {
  const i = r.relPath.lastIndexOf('/')
  return i > 0 ? r.relPath.slice(0, i) : ''
}

async function runSearch() {
  const kw = q.value.trim()
  if (!kw) {
    rows.value = []
    searched.value = false
    return
  }
  loading.value = true
  try {
    const res = await ipc.searchFiles({ q: kw, scope: 'all', limit: 20 })
    rows.value = res.items
    searched.value = true
    cursor.value = 0
  } finally {
    loading.value = false
  }
}

let timer: ReturnType<typeof setTimeout> | undefined
function onInput() {
  clearTimeout(timer)
  timer = setTimeout(runSearch, 80)
}

async function show() {
  open.value = true
  q.value = ''
  rows.value = []
  searched.value = false
  void nextTick(() => inputEl.value?.focus())
}

function hide() {
  open.value = false
  clearTimeout(timer)
}

function move(delta: number) {
  if (rows.value.length === 0) return
  cursor.value = (cursor.value + delta + rows.value.length) % rows.value.length
  void nextTick(() => {
    inputEl.value
      ?.closest('.panel')
      ?.querySelector('.list')
      ?.querySelector('.on')
      ?.scrollIntoView({ block: 'nearest' })
  })
}

async function choose(r: FileRow) {
  hide()
  try {
    await ipc.openPath(livePath(r))
  } catch {
    /* 打不开就回到页面，不弹错 */
  }
}

function goAdvanced() {
  hide()
  router.push({ path: '/search', query: q.value.trim() ? { q: q.value.trim() } : {} })
}

/* ---- 全局键位：Ctrl+K 开合；面板打开时吞掉普通按键 ---- */
function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    e.stopPropagation()
    if (open.value) hide()
    else void show()
    return
  }
  if (!open.value) return
  // 面板是模态的：开着时任何按键都不许漏给整理台 ——
  // 这一条就是「在搜索框里敲 p 把文件搁置了」的解药（T3.3 的红灯）
  e.stopPropagation()
  // 面板开着：在这里处理并拦截，事件不许再传给整理台
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    hide()
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    e.stopPropagation()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    e.stopPropagation()
    move(-1)
  } else if (e.key === 'Enter' && rows.value[cursor.value]) {
    e.preventDefault()
    e.stopPropagation()
    void choose(rows.value[cursor.value])
  }
  // 其余按键（字母、数字）放行给输入框 —— 输入框自己 stopPropagation
}

onMounted(() => window.addEventListener('keydown', onKeydown, true))
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown, true)
  clearTimeout(timer)
})

const emptyState = computed(() => searched.value && rows.value.length === 0 && !loading.value)
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="wrap" @click.self="hide">
      <div class="panel" role="dialog" aria-label="快速检索">
        <div class="phead">
          <AppIcon name="info" :size="14" />
          <input
            ref="inputEl"
            v-model="q"
            class="q"
            type="text"
            placeholder="搜文件名或路径…（↑↓ 选，回车打开）"
            spellcheck="false"
            @input="onInput"
          />
          <span class="esc">Esc 关闭</span>
        </div>

        <ul v-if="rows.length > 0" class="list">
          <li
            v-for="(r, i) in rows"
            :key="r.id"
            class="item"
            :class="{ on: i === cursor }"
            @mouseenter="cursor = i"
            @click="choose(r)"
          >
            <AppIcon :name="iconOf(r.kind)" :size="14" />
            <span class="iname ellipsis" :title="r.name">{{ r.name }}</span>
            <span class="idir ellipsis">{{ dirOf(r) }}</span>
            <span class="ikind">{{ kindLabel(r.kind) }}</span>
          </li>
        </ul>

        <p v-else-if="loading" class="hint">正在搜…</p>
        <p v-else-if="emptyState" class="hint">
          没有匹配的文件。
          <button class="link" @click="goAdvanced">去高级筛选</button>
        </p>
        <p v-else class="hint">输入关键词，按文件名与路径找（不搜正文）</p>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.wrap {
  position: fixed;
  inset: 0;
  z-index: 100;
  background: var(--c-mask, rgba(0, 0, 0, 0.28));
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding-top: 12vh;
  animation: fade var(--dur-fast) ease both;
}
@keyframes fade {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
/* 克制的入场：120ms 淡入，不做缩放弹跳 */
.panel {
  width: min(560px, calc(100vw - 48px));
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  overflow: hidden;
  animation: rise var(--dur-fast) ease both;
}
@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

.phead {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 0.5px solid var(--c-border);
  color: var(--c-text-3);
}
.q {
  flex: 1;
  min-width: 0;
  border: 0;
  background: transparent;
  outline: none;
  font-size: 14px;
  font-family: var(--font);
  color: var(--c-text);
}
.q:focus {
  box-shadow: none;
}
.esc {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--c-text-3);
}

.list {
  list-style: none;
  margin: 0;
  padding: 6px;
  max-height: 320px;
  overflow: auto;
}
.item {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--r-item);
  font-size: 12.5px;
  color: var(--c-text);
  cursor: default;
}
.item.on {
  background: var(--c-primary-bg);
}
.item.on:focus-visible {
  outline: 2px solid var(--c-primary);
  outline-offset: -1px;
}
.iname {
  flex: 0 1 auto;
  min-width: 0;
}
.idir {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 11px;
  color: var(--c-text-3);
  text-align: right;
}
.ikind {
  flex: 0 0 auto;
  font-size: 11px;
  color: var(--c-text-2);
}

.hint {
  margin: 0;
  padding: 18px 16px;
  font-size: 12px;
  color: var(--c-text-3);
}
.link {
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 12px;
  padding: 0;
  text-decoration: underline;
  text-underline-offset: 2px;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
