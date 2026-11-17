<script setup lang="ts">
import AppWindow from '../../components/AppWindow.vue'
import Sidebar from '../../components/Sidebar.vue'
</script>

<template>
  <AppWindow title="文件归档">
    <div class="main">
      <Sidebar />
      <div class="content">
        <router-view />
      </div>
    </div>
  </AppWindow>
</template>

<style scoped>
.main {
  display: flex;
  flex: 1;
  min-height: 0;
}
.content {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
</style>
