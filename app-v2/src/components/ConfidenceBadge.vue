<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{ value: number | null; compact?: boolean }>(),
  { value: null, compact: false }
)

const level = computed(() => {
  if (props.value === null) return 'low'
  if (props.value >= 0.85) return 'high'
  if (props.value >= 0.7) return 'mid'
  return 'low'
})

const text = computed(() =>
  props.value === null ? '—' : `${Math.round(props.value * 100)}%`
)
</script>

<template>
  <span class="badge" :class="[level, { compact }]">{{ text }}</span>
</template>

<style scoped>
.badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 42px;
  padding: 3px 10px;
  border-radius: var(--r-badge);
  font-size: 12px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
}
.badge.compact {
  min-width: 34px;
  padding: 2px 7px;
  font-size: 11px;
}
.high {
  background: var(--c-success-bg);
  color: var(--c-success-deep);
}
.mid {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}
.low {
  background: var(--c-danger-bg);
  color: var(--c-danger);
}
</style>
