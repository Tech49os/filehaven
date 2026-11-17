<script setup lang="ts">
withDefaults(
  defineProps<{
    label: string
    value: string | number
    unit?: string
    tone?: 'default' | 'warning' | 'success' | 'primary'
  }>(),
  { unit: '', tone: 'default' }
)
</script>

<template>
  <div class="stat">
    <div class="label">{{ label }}</div>
    <div class="value" :class="tone">
      {{ value }}<span v-if="unit" class="unit">{{ unit }}</span>
    </div>
    <div v-if="$slots.sub" class="sub"><slot name="sub" /></div>
  </div>
</template>

<style scoped>
.stat {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  padding: 14px 16px;
  min-width: 0;
}
.label {
  font-size: 12px;
  color: var(--c-text-2);
}
.value {
  font-size: 26px;
  font-weight: 500;
  letter-spacing: -0.3px;
  line-height: 1.25;
  color: var(--c-text);
  margin-top: 2px;
}
.value.warning {
  color: var(--c-warning);
}
.value.success {
  color: var(--c-success);
}
.value.primary {
  color: var(--c-primary);
}
.unit {
  font-size: 13px;
  font-weight: 400;
  color: var(--c-text-2);
  margin-left: 3px;
}
.sub {
  font-size: 11.5px;
  color: var(--c-text-3);
  margin-top: 2px;
}
</style>
