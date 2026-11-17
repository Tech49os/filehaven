<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const ready = ref(false)

onMounted(async () => {
  // 从确认页返回时不要重跑（会丢掉用户已经改好的方案）
  if (w.proposal) {
    ready.value = true
    return
  }
  try {
    await w.runProposal()
  } finally {
    ready.value = true
  }
})

const topicCount = computed(() => w.proposal?.topics.length ?? 0)
const ungrouped = computed(() => w.proposal?.ungroupedIds.length ?? 0)
const fileCount = computed(() => w.scannedCount)
</script>

<template>
  <div class="ind">
    <div class="ind-box">
      <template v-if="!ready">
        <div class="ind-spin"></div>
        <div class="ind-title">正在从文件名里归纳事项…</div>
        <div class="ind-sub">
          不预设任何行业词表，完全靠你这批文件的命名规律归纳。几百毫秒即可完成。
        </div>
      </template>

      <template v-else-if="w.error">
        <div class="ind-title warn">没能生成分类方案</div>
        <div class="ind-sub">{{ w.error }}</div>
        <button class="ind-btn" @click="w.runProposal()">重试</button>
      </template>

      <template v-else>
        <div class="ind-done">
          <div class="ind-num">{{ topicCount }}</div>
          <div class="ind-title">个事项</div>
        </div>
        <div class="ind-sub">
          从 {{ fileCount }} 份文件里归纳出 {{ topicCount }} 个事项，
          {{ ungrouped }} 份没能归类。下一步可以改名字、删掉不需要的、把重复的合并。
        </div>
        <button class="ind-btn primary" @click="w.next()">查看并确认分类方案</button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.ind {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}
.ind-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  max-width: 520px;
  text-align: center;
}
.ind-spin {
  width: 28px;
  height: 28px;
  border: 2px solid var(--c-border);
  border-top-color: var(--c-primary);
  border-radius: 50%;
  animation: spin 0.9s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
.ind-done {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.ind-num {
  font-size: 40px;
  font-weight: 500;
  color: var(--c-primary);
  line-height: 1;
}
.ind-title {
  font-size: 16px;
  font-weight: 500;
  color: var(--c-text);
}
.ind-title.warn {
  color: var(--c-warning);
}
.ind-sub {
  font-size: 13px;
  line-height: 1.8;
  color: var(--c-text-2);
}
.ind-btn {
  margin-top: 8px;
  font-size: 13px;
  padding: 9px 22px;
  border-radius: 8px;
  cursor: pointer;
  border: 0.5px solid var(--c-border);
  background: var(--c-surface);
  color: var(--c-text);
}
.ind-btn.primary {
  background: var(--c-primary);
  border-color: var(--c-primary);
  color: #fff;
}
</style>
