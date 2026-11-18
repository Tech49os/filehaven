<script setup lang="ts">
import Card from '../../components/Card.vue'
import AppIcon from '../../components/AppIcon.vue'
</script>

<template>
  <Card title="待迁移">
    <div class="ph">
      <AppIcon name="layers" :size="28" />
      <p class="t">迁移清单在 M7 接入</p>
      <p class="d">
        确认过的文件会汇总到这里，按目录分组、列出同名冲突，再看一眼就能搬。
        搬之前会先检查源文件有没有被改过。
      </p>
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
