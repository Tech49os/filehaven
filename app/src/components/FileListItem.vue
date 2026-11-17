<script setup lang="ts">
import { computed } from 'vue'
import type { FileItem } from '../core/types'
import ConfidenceBadge from './ConfidenceBadge.vue'

const props = withDefaults(
  defineProps<{
    file: FileItem
    subtitle?: string
    showBadge?: boolean
    warn?: boolean
  }>(),
  { subtitle: '', showBadge: true, warn: false }
)

const TYPE_LABEL: Record<string, string> = {
  doc: 'DOC',
  xls: 'XLS',
  ppt: 'PPT',
  pdf: 'PDF',
  image: 'IMG',
  drawio: 'IO',
  text: 'TXT',
  code: 'CODE',
  other: 'FILE',
}

const TYPE_CLASS: Record<string, string> = {
  doc: 't-primary',
  xls: 't-primary',
  ppt: 't-warning',
  pdf: 't-danger',
  drawio: 't-indigo',
  image: 't-indigo',
}

const label = computed(() => TYPE_LABEL[props.file.kind] ?? 'FILE')
const cls = computed(() => TYPE_CLASS[props.file.kind] ?? 't-neutral')

const sub = computed(() => {
  if (props.subtitle) return props.subtitle
  // draw.io 显示识别出的节点文字（差异化可视化）
  if (props.file.drawioNodes?.length) {
    return `识别节点：${props.file.drawioNodes.slice(0, 5).join(' · ')}`
  }
  if (props.file.textSummary) {
    return props.file.textSummary.slice(0, 62)
  }
  return props.file.path
})

/** 结论来源标记：让「这一条是大模型判的」一眼可见 */
const AI_TAG: Record<string, string> = {
  agree: '一致',
  llm: '模型',
  conflict: '冲突',
}
const aiTag = computed(() =>
  props.file.aiSource ? AI_TAG[props.file.aiSource] ?? '' : ''
)
</script>

<template>
  <div class="item" :class="{ warn: warn || file.needReview }">
    <div class="ico" :class="cls">{{ label }}</div>
    <div class="main">
      <div class="name ellipsis">{{ file.name }}</div>
      <div class="sub ellipsis">{{ sub }}</div>
    </div>
    <slot name="tail">
      <span
        v-if="aiTag"
        class="ai-tag"
        :class="'ai-' + file.aiSource"
        :title="file.reason"
      >
        {{ aiTag }}
      </span>
      <ConfidenceBadge v-if="showBadge" :value="file.confidence" />
    </slot>
  </div>
</template>

<style scoped>
.item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  border-bottom: 0.5px solid var(--c-divider);
  min-width: 0;
}
.item:last-child {
  border-bottom: none;
}
.item.warn {
  background: rgba(255, 59, 48, 0.03);
}

.ico {
  flex: 0 0 34px;
  width: 34px;
  height: 34px;
  border-radius: 9px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10.5px;
  font-weight: 500;
  letter-spacing: 0.2px;
}
.t-primary {
  background: var(--c-primary-bg);
  color: var(--c-primary);
}
.t-indigo {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}
.t-warning {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}
.t-danger {
  background: var(--c-danger-bg);
  color: var(--c-danger);
}
.t-neutral {
  background: rgba(142, 142, 147, 0.12);
  color: var(--c-text-2);
}

.main {
  flex: 1;
  min-width: 0;
}
.name {
  font-size: 13px;
  font-weight: 500;
}
.sub {
  font-size: 12px;
  color: var(--c-text-2);
  margin-top: 1px;
}
.ai-tag {
  flex: 0 0 auto;
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 4px;
  margin-right: 6px;
  white-space: nowrap;
}
.ai-agree {
  color: #0a7d3f;
  background: rgba(52, 199, 89, 0.14);
}
.ai-llm {
  color: var(--c-primary);
  background: rgba(0, 122, 255, 0.1);
}
.ai-conflict {
  color: #a86700;
  background: rgba(255, 149, 0, 0.16);
}
</style>
