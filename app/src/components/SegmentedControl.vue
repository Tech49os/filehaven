<script setup lang="ts">
defineProps<{
  modelValue: string
  options: { label: string; value: string }[]
}>()
const emit = defineEmits<{ 'update:modelValue': [string] }>()
</script>

<template>
  <div class="seg">
    <button
      v-for="o in options"
      :key="o.value"
      class="item"
      :class="{ on: o.value === modelValue }"
      @click="emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>

<style scoped>
.seg {
  display: inline-flex;
  background: rgba(0, 0, 0, 0.04);
  border-radius: 8px;
  padding: 2px;
  gap: 2px;
}
.item {
  padding: 4px 12px;
  border-radius: 6px;
  font-size: 12px;
  color: var(--c-text-2);
  transition: all 0.15s ease;
  white-space: nowrap;
}
.item.on {
  background: var(--c-surface);
  color: var(--c-text);
  box-shadow: var(--sh-control);
  font-weight: 500;
}
</style>
