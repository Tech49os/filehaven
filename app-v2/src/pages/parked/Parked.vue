<script setup lang="ts">
import Card from '../../components/Card.vue'
import AppIcon from '../../components/AppIcon.vue'
import { useAppStore } from '../../stores/app'

const app = useAppStore()
</script>

<template>
  <Card title="待定区">
    <div class="ph">
      <AppIcon name="clock" :size="28" />
      <p class="t">待定操作在 P2 接入</p>
      <p class="d">
        在整理台按 <kbd>P</kbd> 「先放着」的文件会进这里，现在有
        <b>{{ app.parkedCount }}</b> 份。P2 会给它们列出候选目录，可以一次指定或直接丢掉。
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
  max-width: 440px;
  text-align: center;
  font-size: 12px;
  line-height: 1.7;
}
kbd {
  font-family: var(--font);
  font-size: 11px;
  padding: 1px 5px;
  border-radius: 5px;
  border: 0.5px solid var(--c-border-strong);
  background: var(--c-field);
}
</style>
