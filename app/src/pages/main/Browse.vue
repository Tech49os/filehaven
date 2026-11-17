<script setup lang="ts">
import { computed, ref } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import CategoryTree from '../../components/CategoryTree.vue'
import FileListItem from '../../components/FileListItem.vue'
import SegmentedControl from '../../components/SegmentedControl.vue'
import { useDataStore } from '../../stores/data'

const data = useDataStore()
const selected = ref<string | null>(null)
const q = ref('')
const mode = ref('all')

const visible = computed(() => {
  let list = data.files
  if (selected.value) {
    const ids = new Set<string>([selected.value])
    data.categories
      .filter((c) => c.parentId === selected.value)
      .forEach((c) => ids.add(c.id))
    list = list.filter((f) => f.categoryId && ids.has(f.categoryId))
  }
  const s = q.value.trim().toLowerCase()
  if (s) {
    list = list.filter(
      (f) =>
        f.name.toLowerCase().includes(s) ||
        f.textSummary.toLowerCase().includes(s) ||
        (f.drawioNodes ?? []).some((n) => n.toLowerCase().includes(s))
    )
  }
  if (mode.value === 'confidence') list = [...list].sort((a, b) => a.confidence - b.confidence)
  return list
})

const title = computed(() => {
  if (!selected.value) return '全部分类'
  return data.categories.find((c) => c.id === selected.value)?.name ?? '全部分类'
})
</script>

<template>
  <div class="page">
    <header class="top">
      <div>
        <h1 class="page-title">分类浏览</h1>
        <p class="page-sub">
          点分类树按维度筛选，或直接搜文件名、正文、架构图节点
        </p>
      </div>
      <div class="search">
        <AppIcon name="search" :size="14" />
        <input v-model="q" class="search-input" placeholder="搜索文件…" />
      </div>
    </header>

    <div class="grid">
      <div class="panel">
        <CategoryTree :tree="data.categoryTree" :selected="selected" @select="selected = $event" />
      </div>

      <div class="panel">
        <header class="lp-head">
          <span class="lp-title">{{ title }}</span>
          <span class="lp-count">{{ visible.length }} 个文件</span>
          <div class="grow" />
          <SegmentedControl
            v-model="mode"
            :options="[
              { label: '全部', value: 'all' },
              { label: '按置信度', value: 'confidence' },
            ]"
          />
        </header>
        <div class="scroll lp-list">
          <FileListItem
            v-for="f in visible"
            :key="f.id"
            :file="f"
            :subtitle="`${data.categories.find((c) => c.id === f.categoryId)?.name ?? '未分类'} · ${f.suggestedName || f.name}`"
          />
          <div v-if="!visible.length" class="blank">这个条件下没有文件</div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page {
  flex: 1;
  min-height: 0;
  padding: var(--pad-content);
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  flex: 0 0 auto;
}
.search {
  display: flex;
  align-items: center;
  gap: 7px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border-strong);
  border-radius: var(--r-btn);
  padding: 7px 11px;
  box-shadow: var(--sh-control);
  color: var(--c-text-2);
  width: 240px;
}
.search-input {
  border: none;
  background: transparent;
  font-size: 12.5px;
  width: 100%;
}
.grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 225px minmax(0, 1fr);
  gap: 16px;
}
.panel {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.lp-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  border-bottom: 0.5px solid var(--c-border);
  flex: 0 0 auto;
}
.lp-title {
  font-size: 13px;
  font-weight: 500;
}
.lp-count {
  font-size: 11.5px;
  color: var(--c-text-2);
}
.lp-list {
  flex: 1;
  min-height: 0;
}
.blank {
  padding: 40px;
  text-align: center;
  font-size: 12px;
  color: var(--c-text-3);
}
</style>
