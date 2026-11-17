<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import StatCard from '../../components/StatCard.vue'
import { undoBatch } from '../../core/migrator'
import { useDataStore } from '../../stores/data'
import { getRootHandle, useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const data = useDataStore()
const router = useRouter()
const reverting = ref(false)
const reverted = ref(false)

const batchId = computed(() => data.records[0]?.batchId ?? '')
const migratedCount = computed(
  () => data.files.filter((f) => f.status === 'migrated').length
)
const pendingCount = computed(() => data.pendingFiles.length)
const totalSec = computed(() => {
  const recs = data.records
  if (!recs.length) return 0
  return Math.max(1, Math.round((Math.max(...recs.map((r) => r.time)) - Math.min(...recs.map((r) => r.time))) / 1000))
})

async function undo() {
  if (reverting.value || !batchId.value) return
  reverting.value = true
  try {
    await undoBatch(batchId.value, data.records, {
      mode: w.mode,
      rootHandle: getRootHandle(),
    })
    data.markReverted(batchId.value)
    reverted.value = true
  } finally {
    reverting.value = false
  }
}

function enter() {
  data.initialized = true
  router.push('/main/dashboard')
}
</script>

<template>
  <div class="step">
    <div class="step-body done-body">
      <div class="orb" :class="{ undone: reverted }">
        <AppIcon :name="reverted ? 'undo' : 'check'" :size="30" :stroke="2.2" />
      </div>
      <h1>{{ reverted ? '已撤销本次迁移' : '整理完成' }}</h1>
      <p class="lead">
        {{
          reverted
            ? '所有文件已恢复到迁移前的位置，归档目录中的副本已清理。'
            : `文件已按 AI 归纳的分类归档到「${w.archivePath}」，源文件全部收进了隔离区，随时可撤销。`
        }}
      </p>

      <div class="stats">
        <StatCard label="已迁移" :value="reverted ? 0 : migratedCount" unit="个" tone="success" />
        <StatCard label="待确认" :value="pendingCount" unit="个" tone="warning">
          <template #sub>可在主界面继续处理</template>
        </StatCard>
        <StatCard label="本次耗时" :value="totalSec" unit="秒" />
        <StatCard label="数据来源" :value="w.mode === 'real' ? '真实文件' : '示例数据'" />
      </div>

      <div class="dest">
        <AppIcon name="drive" :size="15" />
        <span class="k">归档位置</span>
        <code>{{ w.archivePath }}</code>
      </div>

      <div class="next">
        <AppIcon name="sparkle" :size="14" />
        <span>
          接下来：主界面会显示整理概览；开启「空闲自动整理」后，新文件会在电脑空闲时自动归位。
        </span>
      </div>
    </div>

    <div class="step-foot">
      <button
        class="btn btn-text btn-sm"
        :disabled="reverting || reverted || !batchId"
        @click="undo"
      >
        <AppIcon name="undo" :size="13" />
        {{ reverting ? '撤销中…' : reverted ? '已撤销' : '撤销本次迁移' }}
      </button>
      <div class="spacer" />
      <button class="btn btn-primary" @click="enter">
        进入主界面
        <AppIcon name="arrow-right" :size="14" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.done-body {
  align-items: center;
  justify-content: center;
  text-align: center;
  gap: 8px;
}
.orb {
  width: 68px;
  height: 68px;
  border-radius: 22px;
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 8px 26px rgba(40, 200, 64, 0.2);
  margin-bottom: 6px;
}
.orb.undone {
  background: rgba(142, 142, 147, 0.14);
  color: var(--c-text-2);
  box-shadow: none;
}
h1 {
  font-size: 26px;
  font-weight: 500;
  letter-spacing: -0.4px;
  margin: 0;
}
.lead {
  font-size: 13px;
  color: var(--c-text-2);
  line-height: 1.7;
  margin: 0;
  max-width: 540px;
}

.stats {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  width: 100%;
  margin-top: 12px;
}

.dest {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  background: rgba(0, 0, 0, 0.02);
  border-radius: var(--r-banner);
  color: var(--c-text-2);
  margin-top: 6px;
}
.k {
  font-size: 12px;
}
code {
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-surface);
  padding: 2px 8px;
  border-radius: 6px;
  border: 0.5px solid var(--c-border);
}

.next {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  font-size: 12px;
  color: var(--c-text-2);
  max-width: 560px;
  line-height: 1.65;
  text-align: left;
  margin-top: 4px;
}
</style>
