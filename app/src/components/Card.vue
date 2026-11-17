<script setup lang="ts">
defineProps<{ title?: string; flush?: boolean }>()
</script>

<template>
  <section class="card">
    <header v-if="title || $slots.action" class="head">
      <span class="t">{{ title }}</span>
      <div class="act"><slot name="action" /></div>
    </header>
    <div class="content" :class="{ flush }">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.card {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.head {
  display: flex;
  align-items: center;
  padding: 13px 18px;
  border-bottom: 0.5px solid var(--c-border);
  flex: 0 0 auto;
}

.t {
  font-size: 14px;
  font-weight: 500;
}

.act {
  margin-left: auto;
}

.content {
  padding: 16px 18px;
  min-height: 0;
  overflow: auto;
}

.content.flush {
  padding: 0;
}
</style>
