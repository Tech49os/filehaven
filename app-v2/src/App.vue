<script setup lang="ts">
import AppWindow from './components/AppWindow.vue'
</script>

<template>
  <AppWindow title="文件管家">
    <div class="boot">boot</div>
  </AppWindow>
</template>

<style>
html,
body,
#app {
  height: 100%;
  margin: 0;
  background: transparent;
  font-family: var(--font);
  color: var(--c-text);
}

.boot {
  flex: 1;
  display: grid;
  place-items: center;
  color: var(--c-text-3);
  font-size: 13px;
}
</style>
