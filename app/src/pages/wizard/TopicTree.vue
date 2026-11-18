<script setup lang="ts">
/**
 * 确认页左侧的目录树：大类（归档第一层） → 事项。
 *
 * 只负责「看得清 + 选中」，改名/删除/移动都在右侧清单的工具栏里做——
 * 树上一堆小按钮会让人不敢点，而这里最需要的是「扫一眼就知道要建哪些目录」。
 */
import { ref } from 'vue'
import type { TopicTree } from '../../core/emergence/domain'
import type { ProposalTopic } from '../../core/emergence/proposal'
import { countTier } from '../../core/emergence/view'

const props = defineProps<{
  tree: TopicTree<ProposalTopic>
  weakCount: number
  weakFiles: number
  ungrouped: number
  selected: string
}>()

const emit = defineEmits<{ (e: 'select', key: string): void }>()

/** 收起的大类（默认全展开：新建的目录结构一眼看全才敢确认） */
const collapsed = ref<string[]>([])

function toggle(name: string) {
  const i = collapsed.value.indexOf(name)
  if (i >= 0) collapsed.value.splice(i, 1)
  else collapsed.value.push(name)
}

function open(key: string) {
  emit('select', key)
}
</script>

<template>
  <nav class="tree">
    <div
      v-for="d in tree.domains"
      :key="d.name"
      class="grp"
      :class="{ collapsed: collapsed.includes(d.name) }"
    >
      <div
        class="row dom"
        :class="{ on: selected === `d:${d.name}` }"
        :data-key="`d:${d.name}`"
        @click="open(`d:${d.name}`)"
      >
        <button
          class="caret"
          :aria-label="collapsed.includes(d.name) ? '展开' : '收起'"
          @click.stop="toggle(d.name)"
        >
          ▸
        </button>
        <span class="nm">{{ d.name }}</span>
        <span class="n" :class="countTier(d.fileCount)">{{ d.fileCount }}</span>
      </div>

      <div v-show="!collapsed.includes(d.name)" class="kids">
        <div
          v-for="t in d.topics"
          :key="t.id"
          class="row sub"
          :class="{ on: selected === `t:${t.id}`, empty: !t.fileIds.length }"
          :data-key="`t:${t.id}`"
          @click="open(`t:${t.id}`)"
        >
          <span class="nm">{{ t.name }}</span>
          <span class="n" :class="countTier(t.fileIds.length)">{{ t.fileIds.length }}</span>
        </div>
      </div>
    </div>

    <div v-if="tree.loose.length" class="sect">
      <div class="sect-t">不在大类里</div>
      <div
        v-for="t in tree.loose"
        :key="t.id"
        class="row sub root"
        :class="{ on: selected === `t:${t.id}`, empty: !t.fileIds.length }"
        :data-key="`t:${t.id}`"
        @click="open(`t:${t.id}`)"
      >
        <span class="nm">{{ t.name }}</span>
        <span class="n" :class="countTier(t.fileIds.length)">{{ t.fileIds.length }}</span>
      </div>
    </div>

    <div v-if="weakCount" class="sect">
      <div
        class="row weak"
        :class="{ on: selected === 'weak' }"
        data-key="weak"
        @click="open('weak')"
      >
        <span class="nm">待定</span>
        <span class="tag">人名/单位 · 仅文种</span>
        <span class="n">{{ weakFiles }}</span>
      </div>
    </div>

    <div v-if="ungrouped" class="sect">
      <div
        class="row un"
        :class="{ on: selected === 'un' }"
        data-key="un"
        @click="open('un')"
      >
        <span class="nm">未归类</span>
        <span class="n">{{ ungrouped }}</span>
      </div>
    </div>
  </nav>
</template>

<style scoped>
.tree {
  height: 100%;
  overflow-y: auto;
  padding: 8px 8px 16px;
  font-size: 13px;
}
.row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: var(--r-item);
  cursor: pointer;
  color: var(--c-text);
  min-height: 26px;
}
.row:hover {
  background: var(--c-hover);
}
.row.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
}
.nm {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dom .nm {
  font-weight: 500;
}
.sub {
  padding-left: 26px;
  color: var(--c-text-2);
}
.sub.on {
  color: var(--c-primary);
}
.sub.root {
  padding-left: 8px;
}
.row.empty .nm {
  color: var(--c-text-3);
  text-decoration: line-through;
}
.n {
  flex: 0 0 auto;
  font-variant-numeric: tabular-nums;
  font-size: 12px;
  color: var(--c-text-2);
}
.n.md {
  color: var(--c-text);
}
.n.lg {
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text);
}
.caret {
  flex: 0 0 auto;
  width: 14px;
  padding: 0;
  border: none;
  background: transparent;
  font-size: 10px;
  color: var(--c-text-3);
  cursor: pointer;
  transition: transform 0.12s;
  transform: rotate(90deg);
}
.grp.collapsed .caret {
  transform: rotate(0deg);
}
.sect {
  margin-top: 10px;
  padding-top: 8px;
  border-top: 0.5px solid var(--c-divider);
}
.sect-t {
  padding: 0 8px 4px;
  font-size: 11px;
  color: var(--c-text-3);
}
.tag {
  flex: 0 0 auto;
  font-size: 10.5px;
  padding: 0 6px;
  border-radius: var(--r-pill);
  background: var(--c-hover);
  color: var(--c-text-3);
}
.weak .nm,
.un .nm {
  color: var(--c-text-2);
}
</style>
