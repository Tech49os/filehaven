<script setup lang="ts">
import AppIcon from './AppIcon.vue'

withDefaults(defineProps<{ icon?: string; title: string; desc?: string }>(), {
  icon: 'folder',
})
</script>

<template>
  <div class="empty">
    <div class="ico"><AppIcon :name="icon" :size="24" /></div>
    <p class="t">{{ title }}</p>
    <p v-if="desc" class="d">{{ desc }}</p>
    <div class="acts"><slot /></div>
  </div>
</template>

<style scoped>
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 36px 20px;
  border-radius: var(--r-card);
  border: 1px dashed var(--c-border-strong);
  background: var(--c-surface);
}

.ico {
  width: 44px;
  height: 44px;
  border-radius: 12px;
  display: grid;
  place-items: center;
  background: var(--c-field);
  color: var(--c-text-2);
  margin-bottom: 4px;
}

.t {
  margin: 0;
  font-size: 14px;
  font-weight: 500;
}

.d {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
  text-align: center;
  max-width: 380px;
  line-height: 1.7;
}

.acts {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}
.acts:empty {
  display: none;
}
</style>
