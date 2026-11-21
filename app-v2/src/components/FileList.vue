<script setup lang="ts">
/**
 * 文件列表（P3 T3.4 抽出）：浏览页与高级筛选页共用同一套行渲染。
 *
 * 六格：图标+名称 / 类型 / 大小 / 时间 / 标签 / 状态 —— 与 Browser 原列表一致，
 * 「标签进自己的格，不加第七格」（alpha.5 的教训）。
 * 本组件只读、只发事件；打开/定位等动作由父组件接。
 */
import AppIcon from './AppIcon.vue'
import { formatSize, iconOf, kindLabel, relativeTime, statusLabel } from '../core/format'
import type { FileRow } from '../core/types'

withDefaults(
  defineProps<{
    rows: FileRow[]
    activeId?: number | null
    loading?: boolean
  }>(),
  { activeId: null, loading: false },
)

const emit = defineEmits<{
  (e: 'activate', id: number): void
  (e: 'open', row: FileRow): void
  (e: 'menu', payload: { event: MouseEvent; row: FileRow }): void
  (e: 'tagClick', payload: { tagId: number; event: MouseEvent }): void
}>()
</script>

<template>
  <p v-if="loading" class="hint">正在读列表…</p>
  <p v-else-if="rows.length === 0" class="empty">
    <slot name="empty">没有匹配的文件</slot>
  </p>
  <ul v-else class="list">
    <li
      v-for="r in rows"
      :key="r.id"
      class="row"
      :class="{ on: activeId === r.id }"
      @click="emit('activate', r.id)"
      @dblclick="emit('open', r)"
      @contextmenu="emit('menu', { event: $event, row: r })"
    >
      <span class="cell name">
        <AppIcon :name="iconOf(r.kind)" :size="14" />
        <span class="nm ellipsis" :title="r.name">{{ r.name }}</span>
      </span>
      <span class="cell kind">{{ kindLabel(r.kind) }}</span>
      <span class="cell size">{{ formatSize(r.size) }}</span>
      <span class="cell time">{{ relativeTime(r.mtime) }}</span>
      <span class="cell tags" :title="(r.tags ?? []).map((t) => t.name).join('、')">
        <button
          v-for="t in (r.tags ?? []).slice(0, 3)"
          :key="t.id"
          class="tchip"
          :class="t.kind"
          :title="`点这里去检索含「${t.name}」的文件`"
          @click.stop="emit('tagClick', { tagId: t.id, event: $event })"
        >{{ t.name }}</button>
        <span v-if="(r.tags ?? []).length > 3" class="tchip more">+{{ r.tags!.length - 3 }}</span>
      </span>
      <span class="cell st">{{ statusLabel(r.status) }}</span>
    </li>
  </ul>
</template>

<style scoped>
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-3);
  padding: 16px 4px;
}
.empty {
  display: grid;
  place-items: center;
  gap: 8px;
  padding: 48px 12px;
  color: var(--c-text-3);
  font-size: 13px;
}
.list {
  list-style: none;
  margin: 0;
  padding: 0;
  min-height: 0;
  overflow: auto;
  flex: 1;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
}
.row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 52px 72px 92px minmax(0, 1.1fr) 72px;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 10px;
  font-size: 12.5px;
  border-bottom: 0.5px solid var(--c-border);
  cursor: default;
}
.row:last-child {
  border-bottom: 0;
}
.row:hover {
  background: var(--c-hover);
}
.row.on {
  background: var(--c-primary-bg);
}
.cell {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cell.name {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--c-text);
}
.cell.kind,
.cell.size,
.cell.time,
.cell.st {
  color: var(--c-text-2);
  font-size: 11.5px;
}
.cell.time,
.cell.size {
  font-variant-numeric: tabular-nums;
}
.tchip {
  display: inline-block;
  font-size: 10px;
  line-height: 1.5;
  padding: 0 6px;
  margin-right: 3px;
  border-radius: var(--r-pill);
  background: var(--c-field);
  color: var(--c-text-2);
  border: 0;
  font-family: var(--font);
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
  vertical-align: middle;
}
.tchip.topic {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}
.tchip.user {
  background: transparent;
  border: 1px dashed var(--c-border-strong);
}
.tchip.more {
  background: transparent;
  color: var(--c-text-3);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
