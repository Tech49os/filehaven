<script setup lang="ts">
import { computed, ref } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import SafetyBanner from '../../components/SafetyBanner.vue'
import StatCard from '../../components/StatCard.vue'
import { executeMigration } from '../../core/migrator'
import { isDesktop, migrateFiles } from '../../core/desktop'
import type { MigrationPlan } from '../../core/types'
import { useDataStore } from '../../stores/data'
import { fileHandleMap, getRootHandle, useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const data = useDataStore()

const executing = ref(false)
const doneCount = ref(0)
const totalCount = ref(0)
const currentName = ref('')
const stage = ref('')

/** 组装迁移计划：待确认的文件暂不迁移 */
const plans = computed<MigrationPlan[]>(() => {
  const seen = new Set<string>()
  return data.files.map((f) => {
    const cat = data.categories.find((c) => c.id === f.categoryId)
    const parent = cat?.parentId
      ? data.categories.find((c) => c.id === cat.parentId)
      : cat
    const dirName = parent
      ? cat && cat.parentId
        ? `${parent.name}\\${cat.name}`
        : parent.name
      : ''
    let targetName = f.suggestedName || f.name
    const deferred = !cat || f.needReview

    let conflict = false
    if (!deferred) {
      const key = `${dirName}\\${targetName}`
      if (seen.has(key)) {
        conflict = true
        const dot = targetName.lastIndexOf('.')
        const base = dot > 0 ? targetName.slice(0, dot) : targetName
        const ext = dot > 0 ? targetName.slice(dot) : ''
        let i = 1
        while (seen.has(`${dirName}\\${base}_${i}${ext}`)) i++
        targetName = `${base}_${i}${ext}`
      }
      seen.add(`${dirName}\\${targetName}`)
    }

    return {
      fileId: f.id,
      fileName: f.name,
      sourcePath: f.path,
      targetDir: dirName || '（未分类）',
      targetName,
      conflict,
      deferred,
    }
  })
})

const active = computed(() => plans.value.filter((p) => !p.deferred))
const deferredPlans = computed(() => plans.value.filter((p) => p.deferred))
const totalBytes = computed(() =>
  data.files
    .filter((f) => active.value.some((p) => p.fileId === f.id))
    .reduce((a, f) => a + f.size, 0)
)
const conflicts = computed(() => active.value.filter((p) => p.conflict).length)

const mb = computed(() => (totalBytes.value / 1024 / 1024).toFixed(1))

async function start() {
  if (executing.value) return
  executing.value = true
  totalCount.value = active.value.length
  doneCount.value = 0
  const batchId = `b_${Date.now()}`
  try {
    // 桌面版：交给 Rust 做真正的三段式（复制 → SHA256 → 隔离区）
    if (isDesktop && w.mode === 'real') {
      stage.value = '准备中'
      await w.ensureArchive()
      const rep = await migrateFiles(
        w.archivePath,
        batchId,
        active.value.map((p) => ({
          fileId: p.fileId,
          fileName: p.fileName,
          sourcePath: p.sourcePath,
          targetDir: p.targetDir,
          targetName: p.targetName,
        })),
        (done, total, name, st) => {
          doneCount.value = done
          totalCount.value = total
          currentName.value = name
          stage.value = st
        }
      )
      data.addRecords(rep.records)
      const okIds = rep.records
        .filter((r) => r.status === 'recycled' || r.status === 'verified')
        .map((r) => r.fileId)
      data.markMigrated(okIds)
      data.settings.rootPath = w.archivePath
      data.settings.rootName = '已归档'
      data.settings.mode = 'real'
      w.goto(7)
      return
    }

    // 浏览器/演示模式
    const report = await executeMigration(active.value, {
      mode: w.mode,
      rootHandle: getRootHandle(),
      fileHandles: fileHandleMap(),
      batchId,
      onProgress: (done, total, name, st) => {
        doneCount.value = done || doneCount.value
        if (total) totalCount.value = total
        currentName.value = name
        stage.value = st
      },
    })
    data.addRecords(report.records)
    const okIds = report.records
      .filter((r) => r.status === 'recycled' || r.status === 'verified')
      .map((r) => r.fileId)
    data.markMigrated(okIds)
    data.settings.rootName = w.mode === 'real' ? '已归档' : '归档'
    data.settings.rootPath = w.archivePath
    w.goto(7)
  } catch {
    stage.value = '执行出错，已保留全部原文件'
  } finally {
    executing.value = false
  }
}
</script>

<template>
  <div class="step">
    <div class="step-body">
      <div class="head">
        <div>
          <h1>{{ executing ? '正在安全迁移' : '预览迁移计划' }}</h1>
          <p class="sub">
            {{
              executing
                ? '每个文件都走完 复制 → 校验 → 隔离 三步，中途不会丢数据'
                : `确认无误后开始。${deferredPlans.length} 个待确认文件会留在原处。`
            }}
          </p>
        </div>
      </div>

      <SafetyBanner />

      <!-- 执行进度 -->
      <div v-if="executing" class="exec">
        <div class="ex-top">
          <span class="spinner" />
          <span class="ex-stage">{{ stage }}</span>
          <span class="ex-count">{{ doneCount }} / {{ totalCount }}</span>
        </div>
        <div class="ex-bar">
          <div
            class="ex-fill"
            :style="{ width: totalCount ? (doneCount / totalCount) * 100 + '%' : '0%' }"
          />
        </div>
        <div class="ex-name ellipsis">{{ currentName }}</div>
      </div>

      <!-- 对照表 -->
      <div v-else class="panel table-panel">
        <div class="thead">
          <span class="c-src">源文件</span>
          <span class="c-arrow" />
          <span class="c-dst">归档到 {{ w.archivePath }}</span>
        </div>
        <div class="scroll tbody">
          <div v-for="p in plans" :key="p.fileId" class="trow" :class="{ off: p.deferred }">
            <div class="c-src">
              <div class="ellipsis">{{ p.fileName }}</div>
              <div v-if="p.conflict" class="conflict">
                <AppIcon name="info" :size="11" />
                同名冲突 · 已自动加序号，不覆盖
              </div>
            </div>
            <div class="c-arrow">
              <AppIcon name="arrow-right" :size="13" />
            </div>
            <div class="c-dst">
              <template v-if="p.deferred">
                <span class="muted">留在待确认队列</span>
              </template>
              <template v-else>
                <span class="dir">{{ p.targetDir }}\</span>
                <span class="fname">{{ p.targetName }}</span>
              </template>
            </div>
          </div>
        </div>
      </div>

      <div v-if="!executing" class="stats">
        <StatCard label="本次迁移" :value="active.length" unit="个文件" tone="primary" />
        <StatCard label="预计占用" :value="mb" unit="MB" />
        <StatCard
          label="待确认"
          :value="deferredPlans.length"
          unit="个"
          :tone="deferredPlans.length ? 'warning' : 'default'"
        />
        <StatCard
          label="同名冲突"
          :value="conflicts"
          unit="个"
          :tone="conflicts ? 'warning' : 'default'"
        >
          <template #sub>已自动加序号</template>
        </StatCard>
      </div>
    </div>

    <div class="step-foot">
      <button class="btn btn-text btn-sm" :disabled="executing" @click="w.goto(4)">
        <AppIcon name="arrow-left" :size="13" />
        返回微调
      </button>
      <div class="spacer" />
      <span class="muted tiny">可在「迁移记录」一键撤销</span>
      <button class="btn btn-success" :disabled="executing || !active.length" @click="start">
        <AppIcon name="shield" :size="14" />
        {{ executing ? '迁移中…' : '开始安全迁移' }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.head {
  flex: 0 0 auto;
}
h1 {
  font-size: 22px;
  font-weight: 500;
  letter-spacing: -0.3px;
  margin: 0;
}
.sub {
  font-size: 12.5px;
  color: var(--c-text-2);
  margin: 5px 0 0;
}

.panel {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.thead {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 24px minmax(0, 1.2fr);
  gap: 10px;
  padding: 10px 16px;
  border-bottom: 0.5px solid var(--c-border);
  font-size: 11.5px;
  color: var(--c-text-2);
  font-weight: 500;
  flex: 0 0 auto;
  background: rgba(0, 0, 0, 0.012);
}

.tbody {
  flex: 1;
  min-height: 0;
}

.trow {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 24px minmax(0, 1.2fr);
  gap: 10px;
  align-items: center;
  padding: 9px 16px;
  border-bottom: 0.5px solid var(--c-divider);
  font-size: 12.5px;
}
.trow:last-child {
  border-bottom: none;
}
.trow.off {
  opacity: 0.55;
}

.c-arrow {
  color: var(--c-text-3);
  display: flex;
  align-items: center;
  justify-content: center;
}
.c-dst {
  min-width: 0;
}
.dir {
  color: var(--c-text-2);
  font-size: 12px;
}
.fname {
  color: var(--c-primary);
  font-size: 12px;
}
.conflict {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--c-warning-deep);
  margin-top: 2px;
}

.stats {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  flex: 0 0 auto;
}

/* 执行进度 */
.exec {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  padding: 22px;
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 12px;
}
.ex-top {
  display: flex;
  align-items: center;
  gap: 10px;
}
.ex-stage {
  font-size: 13px;
  font-weight: 500;
}
.ex-count {
  margin-left: auto;
  font-size: 12px;
  color: var(--c-text-2);
  font-variant-numeric: tabular-nums;
}
.ex-bar {
  height: 6px;
  background: rgba(0, 0, 0, 0.06);
  border-radius: 4px;
  overflow: hidden;
}
.ex-fill {
  height: 100%;
  background: linear-gradient(90deg, #4cd964, var(--c-success));
  border-radius: 4px;
  transition: width 0.25s ease;
}
.ex-name {
  font-size: 12px;
  color: var(--c-text-2);
}

.step-body {
  gap: 12px;
}
</style>
