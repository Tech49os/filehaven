<script setup lang="ts">
const props = withDefaults(
  defineProps<{ step: number; total?: number; name?: string }>(),
  { total: 7, name: '' }
)

function stateOf(i: number) {
  if (i < props.step) return 'done'
  if (i === props.step) return 'now'
  return 'todo'
}
</script>

<template>
  <div class="stepper">
    <div class="dots">
      <span
        v-for="i in total"
        :key="i"
        class="dot"
        :class="[stateOf(i), { last: i === total }]"
      />
    </div>
    <span class="label">第 {{ step }} 步 · {{ name }}</span>
  </div>
</template>

<style scoped>
.stepper {
  display: flex;
  align-items: center;
  gap: 12px;
}
.dots {
  display: flex;
  align-items: center;
  gap: 6px;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #d2d2d7;
  transition: all 0.22s ease;
}
.dot.done {
  background: var(--c-primary);
}
.dot.now {
  width: 22px;
  border-radius: 4px;
  background: var(--c-primary);
}
.dot.now.last {
  background: var(--c-success);
}
.label {
  font-size: 12px;
  color: var(--c-text-2);
}
</style>
