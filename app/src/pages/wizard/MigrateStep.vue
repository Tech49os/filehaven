<script setup lang="ts">
import { computed, ref } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import SafetyBanner from '../../components/SafetyBanner.vue'
import StatCard from '../../components/StatCard.vue'
import { executeMigration } from '../../core/migrator'
import { isDesktop, migrateFiles } from '../../core/desktop'
import { buildPlans } from '../../core/plan'
import { parseName } from '../../core/version'
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

const plans = computed(() => buildPlans(data.files, data.categories))

const active = computed(() => plans.value.filter((p) => !p.deferred))
const deferredPlans = computed(() => plans.value.filter((p) => p.deferred))
const totalBytes = computed(() =>
  data.files
    .filter((f) => active.value.some((p) => p.fileId === f.id))
    .reduce((a, f) => a + f.size, 0)
)
const conflicts = computed(() => active.value.filter((p) => p.conflict).length)

/** 展开了哪些版本族 */
const openFam = ref(new Set<string>())

interface PlanRow {
  key: string
  title: string
  members: MigrationPlan[]
}

/**
 * 迁移清单的行：同一目录、同一主名的多个版本折成一行。
 * 折叠只为了「看得清」，不改变迁移行为——点开是每个版本各自的源 → 目标。
 */
const planRows = computed<PlanRow[]>(() => {
  const keyOf = (p: MigrationPlan) =>
    `${p.targetDir}\\${parseName(p.fileName).key || p.fileName}`

  const buckets = new Map<string, MigrationPlan[]>()
  for (const p of plans.value) {
    if (p.deferred) continue
    const k = keyOf(p)
    const arr = buckets.get(k)
    if (arr) arr.push(p)
    else buckets.set(k, [p])
  }

  const used = new Set<string>()
  const rows: PlanRow[] = []
  for (const p of plans.value) {
    const k = keyOf(p)
    const members = buckets.get(k)
    if (p.deferred || !members || members.length < 2) {
      rows.push({ key: p.fileId, title: p.fileName, members: [p] })
      continue
    }
    if (used.has(k)) continue
    used.add(k)
    rows.push({ key: k, title: parseName(members[0].fileName).title || members[0].fileName, members })
  }
  return rows
})

const famCount = computed(() => planRows.value.filter((r) => r.members.length > 1).length)

function toggleFam(key: string) {
  const next = new Set(openFam.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  openFam.value = next
}

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
          <template v-for="r in planRows" :key="r.key">
            <!-- 单份：一行到底 -->
            <div v-if="r.members.length === 1" class="trow" :class="{ off: r.members[0].deferred }">
              <div class="c-src">
                <div class="ellipsis">{{ r.members[0].fileName }}</div>
                <div v-if="r.members[0].conflict" class="conflict">
                  <AppIcon name="info" :size="11" />
                  同名冲突 · 已自动加序号，不覆盖
                </div>
                <div v-else-if="r.members[0].renamed" class="tidy">
                  <AppIcon name="info" :size="11" />
                  文件名已规范化
                </div>
                <div v-if="w.dupIndex[r.members[0].fileId]?.length" class="tidy">
                  <AppIcon name="info" :size="11" />
                  与另 {{ w.dupIndex[r.members[0].fileId].length }} 份内容完全相同（都归档）
                </div>
              </div>
              <div class="c-arrow">
                <AppIcon name="arrow-right" :size="13" />
              </div>
              <div class="c-dst">
                <template v-if="r.members[0].deferred">
                  <span class="muted">留在待确认队列</span>
                </template>
                <template v-else>
                  <span class="dir">{{ r.members[0].targetDir }}\</span>
                  <span class="fname">{{ r.members[0].targetName }}</span>
                </template>
              </div>
            </div>

            <!-- 版本族：一行收起，点开看每一版的落点 -->
            <template v-else>
              <div class="trow fam" role="button" tabindex="0" @click="toggleFam(r.key)" @keydown.enter.prevent="toggleFam(r.key)">
                <div class="c-src">
                  <div class="ellipsis">
                    <span class="caret" :class="{ on: openFam.has(r.key) }">›</span>
                    {{ r.title }}
                    <span class="fam-badge">{{ r.members.length }} 个版本</span>
                  </div>
                  <div class="fam-hint">
                    同一份文档的多个版本会一起归档，文件名按版本号统一编排；点开可逐个核对
                  </div>
                </div>
                <div class="c-arrow">
                  <AppIcon name="arrow-right" :size="13" />
                </div>
                <div class="c-dst">
                  <span class="dir">{{ r.members[0].targetDir }}\</span>
                  <span class="fname">{{ r.members[0].targetName }}</span>
                  <span v-if="r.members.length > 1" class="more">等 {{ r.members.length }} 个</span>
                </div>
              </div>
              <div v-if="openFam.has(r.key)" class="fam-kids">
                <div v-for="p in r.members" :key="p.fileId" class="trow kid">
                  <div class="c-src">
                    <div class="ellipsis" :title="p.fileName">{{ p.fileName }}</div>
                  </div>
                  <div class="c-arrow">
                    <AppIcon name="arrow-right" :size="11" />
                  </div>
                  <div class="c-dst">
                    <span class="fname">{{ p.targetName }}</span>
                  </div>
                </div>
              </div>
            </template>
          </template>
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
        <StatCard label="版本族" :value="famCount" unit="组">
          <template #sub>同名版本一起归档</template>
        </StatCard>
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
.tidy {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--c-text-3);
  margin-top: 2px;
}

/* 版本族折叠 */
.trow.fam {
  cursor: pointer;
  background: rgba(0, 0, 0, 0.008);
}
.trow.fam:hover {
  background: var(--c-hover);
}
.caret {
  display: inline-block;
  width: 10px;
  color: var(--c-text-3);
  font-size: 11px;
  transition: transform 0.12s ease;
  transform-origin: 50% 50%;
}
.caret.on {
  transform: rotate(90deg);
}
.fam-badge {
  margin-left: 6px;
  font-size: 10.5px;
  color: var(--c-text-2);
  background: var(--c-hover);
  border-radius: 4px;
  padding: 1px 5px;
}
.fam-hint {
  font-size: 10.5px;
  color: var(--c-text-3);
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.more {
  margin-left: 6px;
  font-size: 10.5px;
  color: var(--c-text-3);
}
.trow.kid {
  padding: 5px 16px 5px 30px;
  background: var(--c-titlebar);
  border-bottom: 0.5px solid var(--c-divider);
}
.trow.kid .c-src div {
  font-size: 11.5px;
  color: var(--c-text-2);
}
.trow.kid .c-dst .fname {
  font-size: 11.5px;
}

.stats {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
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
