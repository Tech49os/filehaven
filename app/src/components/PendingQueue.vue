<script setup lang="ts">
import type { FileItem } from '../core/types'
import ConfidenceBadge from './ConfidenceBadge.vue'
import { useDataStore } from '../stores/data'

defineProps<{
  files: FileItem[]
  selected: string | null
  explain?: string
}>()
const emit = defineEmits<{ select: [string] }>()

const data = useDataStore()

function guessOf(f: FileItem): string {
  if (!f.categoryId) return '无法判断'
  const c = data.categories.find((x) => x.id === f.categoryId)
  if (!c) return '无法判断'
  if (c.parentId) {
    const parent = data.categories.find((x) => x.id === c.parentId)
    return `${parent?.name ?? ''} / ${c.name}`
  }
  return c.name
}
</script>

<template>
  <div class="queue">
    <div class="head">
      <span class="t">待确认</span>
      <span class="n">{{ files.length }}</span>
    </div>
    <div v-if="explain" class="explain">{{ explain }}</div>
    <div class="scroll list">
      <button
        v-for="f in files"
        :key="f.id"
        class="row"
        :class="{ on: selected === f.id }"
        @click="emit('select', f.id)"
      >
        <div class="info">
          <div class="nm ellipsis">{{ f.name }}</div>
          <div class="sub ellipsis">猜测：{{ guessOf(f) }}</div>
        </div>
        <ConfidenceBadge :value="f.confidence" compact />
      </button>
      <div v-if="!files.length" class="empty">
        队列已清空，所有文件都处理完了
      </div>
    </div>
  </div>
</template>

<style scoped>
.queue {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 13px 16px;
  border-bottom: 0.5px solid var(--c-border);
  flex: 0 0 auto;
}
.t {
  font-size: 13px;
  font-weight: 500;
}
.n {
  font-size: 11px;
  font-weight: 500;
  color: #fff;
  background: var(--c-warning);
  border-radius: 10px;
  min-width: 18px;
  height: 18px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.explain {
  font-size: 11.5px;
  color: var(--c-text-2);
  padding: 8px 16px;
  background: rgba(0, 0, 0, 0.015);
  border-bottom: 0.5px solid var(--c-divider);
  line-height: 1.5;
  flex: 0 0 auto;
}
.list {
  flex: 1;
  min-height: 0;
  padding: 6px;
}
.row {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--r-item);
  text-align: left;
  transition: background 0.12s ease;
}
.row:hover {
  background: var(--c-hover);
}
.row.on {
  background: var(--c-primary-bg);
}
.info {
  flex: 1;
  min-width: 0;
}
.nm {
  font-size: 12.5px;
  font-weight: 500;
}
.sub {
  font-size: 11px;
  color: var(--c-text-2);
  margin-top: 1px;
}
.empty {
  padding: 26px 16px;
  text-align: center;
  font-size: 12px;
  color: var(--c-text-3);
}
</style>
