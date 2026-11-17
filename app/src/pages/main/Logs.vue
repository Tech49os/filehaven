<script setup lang="ts">
import { computed, ref } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import { undoBatch } from '../../core/migrator'
import { isDesktop, openPath, undoMigration } from '../../core/desktop'
import { useDataStore } from '../../stores/data'
import { getRootHandle, useWizardStore } from '../../stores/wizard'

const data = useDataStore()
const w = useWizardStore()
const open = ref<string | null>(null)
const busy = ref('')
const toast = ref('')

const batches = computed(() => {
  const m = new Map<string, typeof data.records>()
  for (const r of data.records) {
    if (!m.has(r.batchId)) m.set(r.batchId, [])
    m.get(r.batchId)!.push(r)
  }
  return Array.from(m.entries())
    .map(([id, recs]) => ({
      id,
      time: Math.min(...recs.map((r) => r.time)),
      total: recs.length,
      ok: recs.filter((r) => r.status === 'recycled' || r.status === 'verified').length,
      failed: recs.filter((r) => r.status === 'failed').length,
      reverted: recs.every((r) => r.status === 'reverted'),
      bytes: recs.reduce((a, r) => a + r.size, 0),
      records: recs,
    }))
    .sort((a, b) => b.time - a.time)
})

function fmt(t: number) {
  const d = new Date(t)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

function mb(n: number) {
  return (n / 1024 / 1024).toFixed(1)
}

async function undo(id: string) {
  if (busy.value) return
  busy.value = id
  toast.value = ''
  const b = batches.value.find((x) => x.id === id)
  try {
    if (!b) return
    if (isDesktop) {
      // 桌面版：真正从隔离区还原，并删除归档副本
      const root = data.settings.rootPath || w.archivePath
      const r = await undoMigration(root, id, b.records)
      data.markReverted(id)
      toast.value = `已还原 ${r.restored} 个文件${r.failed ? `，${r.failed} 个失败` : ''}`
      return
    }
    await undoBatch(id, b.records, { mode: w.mode, rootHandle: getRootHandle() })
    data.markReverted(id)
    toast.value = '示例模式：已模拟撤销'
  } catch (e) {
    toast.value = `撤销失败：${e instanceof Error ? e.message : e}`
  } finally {
    busy.value = ''
  }
}

async function reveal() {
  const root = data.settings.rootPath || w.archivePath
  if (!isDesktop) return
  try {
    await openPath(root)
  } catch {
    toast.value = '打不开目录，可能尚未创建'
  }
}

const STATUS_LABEL: Record<string, string> = {
  copied: '已复制',
  verified: '已校验',
  recycled: '已迁移',
  reverted: '已撤销',
  failed: '失败',
}
</script>

<template>
  <div class="page scroll">
    <header class="top">
      <div>
        <h1 class="page-title">迁移记录</h1>
        <p class="page-sub">每一批迁移都可回滚，源文件在隔离区保留</p>
      </div>
      <button v-if="w.desktop" class="btn btn-secondary btn-sm" @click="reveal">
        <AppIcon name="drive" :size="12" />
        打开归档目录
      </button>
    </header>

    <div v-if="toast" class="toast">{{ toast }}</div>

    <div v-if="!batches.length" class="blank">
      还没有迁移记录
    </div>

    <div v-for="b in batches" :key="b.id" class="batch">
      <div class="b-head" @click="open = open === b.id ? null : b.id">
        <span class="caret" :class="{ on: open === b.id }">
          <AppIcon name="chevron-right" :size="13" />
        </span>
        <div class="b-main">
          <div class="b-title">
            迁移 {{ b.total }} 个文件
            <span v-if="b.reverted" class="chip grey">已撤销</span>
            <span v-else class="chip green">成功</span>
            <span v-if="b.failed" class="chip red">{{ b.failed }} 个失败</span>
          </div>
          <div class="b-sub">{{ fmt(b.time) }} · {{ mb(b.bytes) }} MB</div>
        </div>
        <button
          class="btn btn-secondary btn-sm"
          :disabled="b.reverted || !!busy"
          @click.stop="undo(b.id)"
        >
          <AppIcon name="undo" :size="12" />
          {{ b.reverted ? '已撤销' : busy === b.id ? '撤销中…' : '撤销' }}
        </button>
      </div>

      <div v-if="open === b.id" class="b-body">
        <div v-for="r in b.records" :key="r.id" class="rec">
          <div class="r-name ellipsis">{{ r.fileName }}</div>
          <div class="r-dst ellipsis">{{ r.targetPath }}</div>
          <span class="st" :class="r.status">{{ STATUS_LABEL[r.status] }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page {
  flex: 1;
  min-height: 0;
  padding: var(--pad-content);
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.top {
  flex: 0 0 auto;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.toast {
  font-size: 12px;
  padding: 9px 14px;
  border-radius: var(--r-banner);
  background: var(--c-primary-bg);
  color: var(--c-primary);
  flex: 0 0 auto;
}
.blank {
  padding: 60px;
  text-align: center;
  color: var(--c-text-3);
  font-size: 12px;
}
.batch {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  overflow: hidden;
  flex: 0 0 auto;
}
.b-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  cursor: pointer;
}
.b-head:hover {
  background: var(--c-hover);
}
.caret {
  color: var(--c-text-3);
  display: flex;
  transition: transform 0.18s ease;
}
.caret.on {
  transform: rotate(90deg);
}
.b-main {
  flex: 1;
  min-width: 0;
}
.b-title {
  font-size: 13px;
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 7px;
}
.b-sub {
  font-size: 11.5px;
  color: var(--c-text-2);
  margin-top: 1px;
}
.chip {
  font-size: 10.5px;
  font-weight: 500;
  border-radius: 6px;
  padding: 2px 7px;
}
.chip.green {
  background: var(--c-success-bg);
  color: var(--c-success-deep);
}
.chip.red {
  background: var(--c-danger-bg);
  color: var(--c-danger);
}
.chip.grey {
  background: rgba(142, 142, 147, 0.14);
  color: var(--c-text-2);
}
.b-body {
  border-top: 0.5px solid var(--c-border);
  padding: 4px 0;
}
.rec {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr) 68px;
  gap: 10px;
  align-items: center;
  padding: 7px 18px;
  font-size: 12px;
  border-bottom: 0.5px solid var(--c-divider);
}
.rec:last-child {
  border-bottom: none;
}
.r-dst {
  color: var(--c-text-2);
  font-size: 11.5px;
}
.st {
  font-size: 11px;
  text-align: right;
}
.st.recycled,
.st.verified {
  color: var(--c-success-deep);
}
.st.reverted {
  color: var(--c-text-2);
}
.st.failed {
  color: var(--c-danger);
}
</style>
