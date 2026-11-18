<script setup lang="ts">
/**
 * 整理台的主卡片（T6.10）
 *
 * 一份文件一屏一个，所有可点的东西都带键盘提示 ——
 * 用户全程不碰鼠标也能走完一批（验收 3）。
 */
import { computed } from 'vue'
import AppIcon from './AppIcon.vue'
import CandidateList from './CandidateList.vue'
import { formatSize, iconOf, kindLabel, relativeTime } from '../core/format'
import type { BatchItem, DecideAck } from '../core/types'

const props = withDefaults(
  defineProps<{
    item: BatchItem
    dirCount: number
    position: { index: number; total: number }
    deferredBadge?: boolean
    ack?: DecideAck | null
    busy?: boolean
  }>(),
  { deferredBadge: false, ack: null, busy: false },
)

const emit = defineEmits<{
  (e: 'accept'): void
  (e: 'pick', n: 1 | 2 | 3): void
  (e: 'newDir'): void
  (e: 'defer'): void
  (e: 'park'): void
}>()

const f = computed(() => props.item.file)
const confirmed = computed(() => f.value.status === 'confirmed')
const handledElsewhere = computed(
  () => f.value.status === 'queued' || f.value.status === 'parked',
)
const noDirs = computed(() => props.dirCount === 0)
const primaryIsNewDir = computed(() => noDirs.value || props.item.candidates.length === 0)

const targetPath = computed(() => {
  if (!confirmed.value) return null
  return props.ack?.targetRelPath ?? null
})
</script>

<template>
  <article class="card" :class="{ done: confirmed, aside: handledElsewhere }">
    <header class="hd">
      <div class="ico"><AppIcon :name="iconOf(f.kind)" :size="18" /></div>
      <div class="nm">
        <h2 class="name" :title="f.name">{{ f.name }}</h2>
        <p class="meta">
          <span>{{ formatSize(f.size) }}</span>
          <span class="dot">·</span>
          <span>{{ kindLabel(f.kind) }}</span>
          <span class="dot">·</span>
          <span>{{ relativeTime(f.mtime) }}</span>
          <span class="dot">·</span>
          <span class="from" :title="f.relPath">{{ f.relPath }}</span>
        </p>
      </div>
      <div class="state">
        <span class="counter">{{ position.index + 1 }} / {{ position.total }}</span>
        <span v-if="deferredBadge" class="badge defer">已暂缓 {{ f.deferCount }} 次</span>
        <span v-if="item.needsYou && !noDirs" class="badge you">待你指定</span>
        <span v-if="confirmed" class="badge ok"><AppIcon name="check" :size="11" /> 已确认</span>
      </div>
    </header>

    <!-- 标签行：P1 只有基础标签（类型/时间），不装样子 -->
    <div class="tags">
      <span class="tag">{{ kindLabel(f.kind) }}</span>
      <span class="tag">{{ relativeTime(f.mtime) }}</span>
      <span v-if="f.deferCount > 0" class="tag warn">暂缓过 {{ f.deferCount }} 次</span>
      <span v-if="f.note" class="tag note">备注：{{ f.note }}</span>
    </div>

    <p v-if="confirmed" class="result">
      <AppIcon name="check" :size="13" />
      已归到 <b>{{ targetPath ?? '所选目录' }}</b>
    </p>
    <p v-else-if="handledElsewhere" class="result dim">
      {{ f.status === 'parked' ? '已放进待定区，回头再定' : '已暂缓，回到池子里下批再说' }}
    </p>

    <div class="body">
      <CandidateList
        v-if="!confirmed && !handledElsewhere"
        :candidates="item.candidates"
        :needs-you="item.needsYou"
        :dir-count="dirCount"
        :disabled="busy"
        @pick="emit('pick', $event)"
      />
    </div>

    <footer class="ft">
      <button
        v-if="!confirmed && !handledElsewhere"
        class="btn primary"
        :disabled="busy"
        @click="primaryIsNewDir ? emit('newDir') : emit('accept')"
      >
        <span class="kbd">{{ primaryIsNewDir ? '回车' : '回车' }}</span>
        {{ primaryIsNewDir ? '新建目录' : `归到 ${item.candidates[0]?.relPath ?? ''}` }}
      </button>
      <button
        v-if="!confirmed && !handledElsewhere && !primaryIsNewDir"
        class="btn"
        :disabled="busy"
        @click="emit('newDir')"
      >
        <span class="kbd">N</span> 新建目录
      </button>
      <button
        v-if="!confirmed && !handledElsewhere"
        class="btn"
        :disabled="busy"
        @click="emit('defer')"
      >
        <span class="kbd">S</span> 暂缓
      </button>
      <button
        v-if="!confirmed && !handledElsewhere"
        class="btn"
        :disabled="busy"
        @click="emit('park')"
      >
        <span class="kbd">P</span> 先放着
      </button>
      <span v-if="confirmed || handledElsewhere" class="hint">按 ↓ 看下一份，Ctrl+Z 撤销</span>
    </footer>
  </article>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  padding: 18px 20px;
  min-height: 0;
}
.card.done {
  border-color: rgba(40, 200, 64, 0.35);
  box-shadow: 0 0 0 3px var(--c-success-bg);
}
.card.aside {
  border-color: rgba(255, 149, 0, 0.3);
}

.hd {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.ico {
  width: 34px;
  height: 34px;
  flex: 0 0 34px;
  border-radius: 9px;
  background: var(--c-field);
  display: grid;
  place-items: center;
  color: var(--c-text-2);
}

.nm {
  min-width: 0;
  flex: 1;
}

.name {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  margin: 3px 0 0;
  font-size: 12px;
  color: var(--c-text-2);
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
}
.meta > span {
  flex: 0 0 auto;
}
.meta .dot {
  color: var(--c-text-3);
}
.meta .from {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.state {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.counter {
  font-size: 11px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  padding: 2px 7px;
  border-radius: var(--r-badge);
  white-space: nowrap;
}
.badge.defer {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}
.badge.you {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}
.badge.ok {
  background: var(--c-success-bg);
  color: var(--c-success-deep);
}

.tags {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.tag {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: var(--r-pill);
  background: var(--c-field);
  color: var(--c-text-2);
}
.tag.warn {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}
.tag.note {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}

.result {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: 9px 12px;
  border-radius: var(--r-banner);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  font-size: 13px;
}
.result.dim {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}

.body {
  min-height: 0;
}

.ft {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  border-top: 0.5px solid var(--c-border);
  padding-top: 12px;
  margin-top: auto;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 0;
  border-radius: var(--r-btn);
  padding: 7px 12px;
  font-size: 12px;
  background: var(--c-field);
  color: var(--c-text);
  font-family: var(--font);
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn:disabled {
  opacity: 0.55;
}

.kbd {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 5px;
  background: rgba(255, 255, 255, 0.22);
  border: 0.5px solid rgba(255, 255, 255, 0.3);
}
.btn:not(.primary) .kbd {
  background: var(--c-surface);
  border-color: var(--c-border-strong);
  color: var(--c-text-2);
}

.hint {
  margin-left: auto;
  font-size: 11px;
  color: var(--c-text-3);
}
</style>
