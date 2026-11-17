<script setup lang="ts">
defineProps<{ title?: string }>()
</script>

<template>
  <div class="win">
    <header class="titlebar">
      <div class="lights" aria-hidden="true">
        <i class="r" /><i class="y" /><i class="g" />
      </div>
      <div class="title">{{ title }}</div>
      <div class="tail"><slot name="tail" /></div>
    </header>
    <main class="body">
      <slot />
    </main>
  </div>
</template>

<style scoped>
.win {
  width: 1180px;
  height: 802px;
  max-width: 100%;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--c-bg);
  border-radius: var(--r-window);
  box-shadow: var(--sh-window);
  border: 0.5px solid var(--c-border-strong);
  overflow: hidden;
}

.titlebar {
  height: 44px;
  flex: 0 0 44px;
  display: flex;
  align-items: center;
  padding: 0 16px;
  background: var(--c-titlebar);
  border-bottom: 0.5px solid var(--c-border);
  backdrop-filter: blur(20px);
  position: relative;
}

.lights {
  display: flex;
  gap: 8px;
  width: 76px;
}
.lights i {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  display: block;
}
.lights .r {
  background: #ff5f57;
}
.lights .y {
  background: #febc2e;
}
.lights .g {
  background: #28c840;
}

.title {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text-2);
  letter-spacing: 0.1px;
}

.tail {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
