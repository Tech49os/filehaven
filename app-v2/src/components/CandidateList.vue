<script setup lang="ts">
import type { Candidate } from '../core/types'

withDefaults(
  defineProps<{
    candidates: Candidate[]
    needsYou?: boolean
    dirCount: number
    disabled?: boolean
  }>(),
  { needsYou: false, disabled: false },
)

const emit = defineEmits<{ (e: 'pick', n: 1 | 2 | 3): void }>()
</script>

<template>
  <div class="wrap">
    <p v-if="dirCount === 0" class="note">
      还没有任何目录，先按 <kbd>N</kbd> 建一个，1/2/3 暂时用不了。
    </p>

    <p v-else-if="needsYou && candidates.length === 0" class="note you">
      没有足够像的目录 —— 这份等你定：按 <kbd>N</kbd> 新建，或 <kbd>S</kbd> 先暂缓。
    </p>

    <ul v-if="candidates.length > 0" class="list">
      <li
        v-for="(c, i) in candidates"
        :key="c.dirId"
        class="cand"
        :class="{ uncertain: c.uncertain, disabled }"
        @click="!disabled && emit('pick', (i + 1) as 1 | 2 | 3)"
      >
        <span class="key">{{ i === 0 ? '回车' : i + 1 }}</span>
        <div class="body">
          <div class="line">
            <span class="path">{{ c.relPath }}</span>
            <span v-if="c.uncertain" class="tag warn">不太确定</span>
          </div>
          <p class="reason">{{ c.reason }}</p>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.wrap {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.cand {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 9px 11px;
  border-radius: var(--r-banner);
  border: 0.5px solid var(--c-border);
  background: var(--c-field);
  cursor: default;
  transition: border-color 0.12s ease, background 0.12s ease;
}
.cand:hover {
  background: var(--c-surface);
  border-color: var(--c-primary);
}
.cand.uncertain {
  background: var(--c-warning-bg);
  border-color: rgba(255, 149, 0, 0.25);
}
.cand.disabled {
  opacity: 0.6;
  pointer-events: none;
}

.key {
  flex: 0 0 auto;
  min-width: 34px;
  height: 20px;
  padding: 0 6px;
  border-radius: 6px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border-strong);
  font-size: 11px;
  color: var(--c-text-2);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-family: var(--font);
}

.body {
  min-width: 0;
  flex: 1;
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.path {
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag.warn {
  flex: 0 0 auto;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: var(--r-badge);
  background: rgba(255, 149, 0, 0.18);
  color: var(--c-warning-deep);
}

.reason {
  margin: 3px 0 0;
  font-size: 12px;
  color: var(--c-text-2);
  line-height: 1.55;
}

.note {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
  padding: 8px 11px;
  border-radius: var(--r-banner);
  background: var(--c-field);
}
.note.you {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}

kbd {
  font-family: var(--font);
  font-size: 11px;
  padding: 1px 5px;
  border-radius: 5px;
  border: 0.5px solid var(--c-border-strong);
  background: var(--c-surface);
  color: var(--c-text);
}
</style>
