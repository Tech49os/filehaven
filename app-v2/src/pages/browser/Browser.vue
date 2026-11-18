<script setup lang="ts">
import Card from '../../components/Card.vue'
import AppIcon from '../../components/AppIcon.vue'
</script>

<template>
  <Card title="浏览">
    <div class="ph">
      <AppIcon name="folder" :size="28" />
      <p class="t">目录浏览在 P2 接入</p>
      <p class="d">P1 的浏览只做「归档区里现在有什么」，左侧目录树 + 右侧文件列表。</p>
    </div>
  </Card>
</template>

<style scoped>
.ph {
  display: grid;
  place-items: center;
  gap: 6px;
  color: var(--c-text-3);
  padding: 40px 12px;
}
.t {
  margin: 6px 0 0;
  font-size: 14px;
  color: var(--c-text-2);
}
.d {
  margin: 0;
  max-width: 420px;
  text-align: center;
  font-size: 12px;
  line-height: 1.7;
}
</style>
