<script setup lang="ts">
import { ref, watch } from 'vue'
import type { CategoryNode } from '../core/types'
import AppIcon from './AppIcon.vue'

const props = defineProps<{
  tree: CategoryNode[]
  selected: string | null
}>()
const emit = defineEmits<{ select: [string | null] }>()

const open = ref<Record<string, boolean>>({})

// 默认展开文件数最多的分类，让二级结构可见
watch(
  () => props.tree,
  (t) => {
    if (t.length && Object.keys(open.value).length === 0) {
      const first = [...t].sort((a, b) => b.totalCount - a.totalCount)[0]
      if (first?.children.length) open.value[first.id] = true
    }
  },
  { immediate: true }
)

function toggle(id: string) {
  open.value[id] = !open.value[id]
}

function pick(id: string | null) {
  emit('select', id)
}
</script>

<template>
  <div class="tree">
    <div class="scroll inner">
      <button class="row all" :class="{ on: selected === null }" @click="pick(null)">
        <AppIcon name="layers" :size="15" />
        <span class="nm">全部分类</span>
      </button>

      <template v-for="node in tree" :key="node.id">
        <button
          class="row"
          :class="{ on: selected === node.id }"
          @click="pick(node.id)"
        >
          <span
            class="caret"
            :class="{ open: open[node.id], hide: !node.children.length }"
            @click.stop="toggle(node.id)"
          >
            <AppIcon name="chevron-right" :size="12" />
          </span>
          <AppIcon name="folder" :size="15" />
          <span class="nm ellipsis">{{ node.name }}</span>
          <span class="cnt">{{ node.totalCount }}</span>
        </button>
        <div v-if="open[node.id]" class="children">
          <button
            v-for="c in node.children"
            :key="c.id"
            class="row sub"
            :class="{ on: selected === c.id }"
            @click="pick(c.id)"
          >
            <span class="nm ellipsis">{{ c.name }}</span>
            <span class="cnt">{{ c.count }}</span>
          </button>
        </div>
      </template>
    </div>

    <div class="legend">
      <div class="lg"><i class="d high" />高置信 ≥85%</div>
      <div class="lg"><i class="d mid" />需留意 70–85%</div>
      <div class="lg"><i class="d low" />待确认 &lt;70%</div>
    </div>
  </div>
</template>

<style scoped>
.tree {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
}
.inner {
  flex: 1;
  padding: 8px;
  min-height: 0;
}
.row {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 9px;
  border-radius: var(--r-item);
  font-size: 13px;
  color: var(--c-text);
  text-align: left;
  transition: background 0.12s ease;
}
.row:hover {
  background: var(--c-hover);
}
.row.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-weight: 500;
}
.row.all {
  margin-bottom: 4px;
}
.nm {
  flex: 1;
  min-width: 0;
}
.cnt {
  font-size: 11.5px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}
.row.on .cnt {
  color: var(--c-primary);
  opacity: 0.75;
}
.caret {
  width: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--c-text-3);
  transition: transform 0.18s ease;
  flex: 0 0 14px;
}
.caret.open {
  transform: rotate(90deg);
}
.caret.hide {
  visibility: hidden;
}
.children {
  padding-left: 20px;
}
.row.sub {
  font-size: 12.5px;
  color: var(--c-text-2);
  padding: 6px 9px;
}
.row.sub.on {
  color: var(--c-primary);
}

.legend {
  border-top: 0.5px solid var(--c-border);
  padding: 10px 14px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  flex: 0 0 auto;
}
.lg {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--c-text-2);
}
.d {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: block;
}
.d.high {
  background: var(--c-success);
}
.d.mid {
  background: var(--c-warning);
}
.d.low {
  background: var(--c-danger);
}
</style>
