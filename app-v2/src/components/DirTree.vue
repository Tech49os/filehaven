<script setup lang="ts">
import AppIcon from './AppIcon.vue'
import type { DirNode } from '../core/types'

withDefaults(
  defineProps<{
    nodes: DirNode[]
    selectedId: number | null
    /** 折叠状态（受控，父组件管） */
    collapsed?: Set<number>
  }>(),
  { collapsed: () => new Set<number>() },
)

const emit = defineEmits<{
  (e: 'select', id: number): void
  (e: 'toggle', id: number): void
}>()

function hasChildren(n: DirNode) {
  return n.children.length > 0
}
</script>

<template>
  <ul class="tree">
    <li v-for="n in nodes" :key="n.id">
      <div
        class="row"
        :class="{ on: selectedId === n.id }"
        :style="{ paddingLeft: `${8 + (n.depth - 1) * 14}px` }"
        role="treeitem"
        :aria-selected="selectedId === n.id"
        :aria-expanded="hasChildren(n) ? !collapsed.has(n.id) : undefined"
        @click="emit('select', n.id)"
      >
        <button
          v-if="hasChildren(n)"
          class="caret"
          :class="{ down: !collapsed.has(n.id) }"
          :aria-label="collapsed.has(n.id) ? '展开' : '收起'"
          @click.stop="emit('toggle', n.id)"
        >
          <AppIcon name="chevron-right" :size="12" />
        </button>
        <span v-else class="caret-space" />
        <AppIcon :name="hasChildren(n) ? 'folder' : 'folder'" :size="14" />
        <span class="name ellipsis">{{ n.name }}</span>
        <span v-if="n.fileCount > 0" class="cnt">{{ n.fileCount }}</span>
      </div>

      <DirTree
        v-if="hasChildren(n) && !collapsed.has(n.id)"
        :nodes="n.children"
        :selected-id="selectedId"
        :collapsed="collapsed"
        @select="emit('select', $event)"
        @toggle="emit('toggle', $event)"
      />
    </li>
  </ul>
</template>

<style scoped>
.tree {
  list-style: none;
  margin: 0;
  padding: 0;
}

.row {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding-right: 8px;
  border-radius: var(--r-item);
  font-size: 13px;
  color: var(--c-text);
  cursor: default;
  user-select: none;
}
.row:hover {
  background: var(--c-hover);
}
.row.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-weight: 500;
}
.row:focus-visible {
  outline: 2px solid var(--c-primary);
  outline-offset: -1px;
}

.caret {
  width: 16px;
  height: 16px;
  display: grid;
  place-items: center;
  border: 0;
  background: transparent;
  color: var(--c-text-2);
  padding: 0;
  transition: transform 0.12s ease;
}
.caret.down {
  transform: rotate(90deg);
}
.caret-space {
  width: 16px;
}

.name {
  flex: 1;
  min-width: 0;
}

.cnt {
  font-size: 11px;
  color: var(--c-text-3);
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
