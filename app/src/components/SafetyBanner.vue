<script setup lang="ts">
import AppIcon from './AppIcon.vue'
</script>

<template>
  <div class="banner">
    <div class="left">
      <span class="check"><AppIcon name="check" :size="12" :stroke="2.6" /></span>
      <div>
        <div class="title">三段式安全迁移</div>
        <div class="note">不直删 · 不覆盖 · 全程可撤销</div>
      </div>
    </div>
    <div class="steps">
      <span class="step">① 复制到目标</span>
      <span class="arrow">→</span>
      <span class="step">② 校验哈希</span>
      <span class="arrow">→</span>
      <span class="step">③ 源送隔离区</span>
    </div>
  </div>
</template>

<style scoped>
.banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 13px 16px;
  border-radius: var(--r-banner);
  background: rgba(40, 200, 64, 0.08);
  border: 0.5px solid rgba(40, 200, 64, 0.25);
  flex-wrap: wrap;
}

.left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.check {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--c-success);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 0 0 20px;
}

.title {
  font-size: 13px;
  font-weight: 500;
  color: #166534;
  line-height: 1.35;
}

.note {
  font-size: 11.5px;
  color: #3f8552;
}

.steps {
  display: flex;
  align-items: center;
  gap: 6px;
}

.step {
  background: #fff;
  border-radius: var(--r-badge);
  padding: 5px 10px;
  font-size: 11.5px;
  color: var(--c-text);
  box-shadow: var(--sh-control);
  white-space: nowrap;
}

.arrow {
  color: rgba(22, 101, 52, 0.5);
  font-size: 11px;
}
</style>
