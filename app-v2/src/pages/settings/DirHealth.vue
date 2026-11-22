<script setup lang="ts">
/**
 * BUG-003 目录体检：手动扫描已有目录，输出可解释、可拒绝的修正建议。
 *
 * 纪律：扫描只在用户点按钮时触发（绝不进 refreshBoot）；采纳只执行 rename/move；
 * empty/merge 只报告不执行；每条建议可逐条采纳/忽略，忽略项不进执行批次。
 */
import { onMounted, onUnmounted, ref } from 'vue'
import * as ipc from '../../core/ipc'
import { onDirHealthStage } from '../../core/events'
import type {
  DirHealthApplyReq,
  DirHealthIssue,
  DirHealthReport,
  DirHealthStage,
} from '../../core/types'
import type { UnlistenFn } from '@tauri-apps/api/event'

const emit = defineEmits<{ (e: 'changed'): void }>()

const busy = ref(false)
const report = ref<DirHealthReport | null>(null)
const stage = ref<DirHealthStage | null>(null)
const err = ref<string | null>(null)
/** 被忽略的建议 id 集合 —— 忽略项绝不进入执行批次 */
const ignored = new Set<string>()
/** 被采纳的建议 id（只标记，不重复提交） */
const adopted = ref<Set<string>>(new Set())
const lastMigrationId = ref<number | null>(null)
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  unlisten = await onDirHealthStage((p) => {
    stage.value = p
  })
})
onUnmounted(() => unlisten?.())

const STAGE_LABEL: Record<string, string> = {
  scan: '正在扫描目录',
  rule: '规则初筛中',
  model: '模型复核中',
  done: '完成',
}

const stageText = () => {
  if (!stage.value) return ''
  const note = stage.value.note?.trim()
  return note || STAGE_LABEL[stage.value.stage] || stage.value.stage
}

/** 这类建议只能看，不能自动执行（非目标约束） */
const canAct = (it: DirHealthIssue) =>
  (it.kind === 'rename' || it.kind === 'move') && !!it.targetPath && !ignored.has(it.id)

const kindLabel = (kind: string) =>
  ({ rename: '改名', move: '调层级', merge: '同义', empty: '空目录' }[kind] ?? kind)

async function scan() {
  busy.value = true
  err.value = null
  report.value = null
  stage.value = null
  ignored.clear()
  adopted.value = new Set()
  try {
    report.value = await ipc.dirHealthScan()
  } catch (e: unknown) {
    err.value = e instanceof Error ? e.message : '体检失败，请稍后再试'
  } finally {
    busy.value = false
  }
}

function ignore(it: DirHealthIssue) {
  ignored.add(it.id)
}

async function adopt(it: DirHealthIssue) {
  if (!canAct(it) || busy.value) return
  busy.value = true
  err.value = null
  const req: DirHealthApplyReq = {
    issueId: it.id,
    kind: it.kind,
    dirId: it.dirId,
    targetPath: it.targetPath!,
  }
  try {
    const ack = await ipc.applyDirHealth(req)
    lastMigrationId.value = ack.migrationId
    const next = new Set(adopted.value)
    next.add(it.id)
    adopted.value = next
    emit('changed')
  } catch (e: unknown) {
    err.value = e instanceof Error ? e.message : '采纳失败，目录未改动'
  } finally {
    busy.value = false
  }
}

async function undo() {
  if (lastMigrationId.value === null || busy.value) return
  busy.value = true
  err.value = null
  try {
    await ipc.undoDirHealth(lastMigrationId.value)
    lastMigrationId.value = null
    report.value = null
    adopted.value = new Set()
    emit('changed')
  } catch (e: unknown) {
    err.value = e instanceof Error ? e.message : '撤销失败'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="dir-health">
    <div class="bar">
      <button class="btn" :disabled="busy" @click="scan">目录体检</button>
      <button class="btn ghost" :disabled="busy || lastMigrationId === null" @click="undo">
        撤销上次修正
      </button>
      <span v-if="stage" class="stage">{{ stageText() }}</span>
    </div>

    <p v-if="err" class="err">{{ err }}</p>

    <template v-if="report">
      <p class="hint">
        共 {{ report.issues.length }} 条建议
        <span v-if="!report.modelReviewed" class="degraded">（未经模型复核：仅规则初筛）</span>
      </p>
      <p v-if="report.note" class="note">{{ report.note }}</p>

      <p v-if="report.issues.length === 0" class="hint dim">没发现明显问题，目录结构挺整齐。</p>

      <ul v-else class="issues">
        <li v-for="it in report.issues" :key="it.id" class="issue" :class="{ done: adopted.has(it.id) || ignored.has(it.id) }">
          <div class="line1">
            <span class="kind" :class="it.kind">{{ kindLabel(it.kind) }}</span>
            <span class="path" :title="it.relPath">{{ it.relPath }}</span>
            <span v-if="it.targetPath" class="target">→ {{ it.targetPath }}</span>
            <span class="conf">{{ Math.round(it.confidence * 100) }}%</span>
          </div>
          <div class="line2">
            <span class="reason">{{ it.reason }}</span>
            <span class="meta">{{ it.affectedFiles }} 份文件 · {{ it.source === 'model' ? '模型确认' : '规则' }}</span>
          </div>
          <div class="acts">
            <template v-if="canAct(it)">
              <button class="btn mini" :disabled="busy" @click="adopt(it)">采纳</button>
              <button class="btn mini ghost" :disabled="busy" @click="ignore(it)">忽略</button>
            </template>
            <span v-else-if="adopted.has(it.id)" class="adopted">已采纳</span>
            <span v-else-if="ignored.has(it.id)" class="ignored-tag">已忽略</span>
            <span v-else class="readonly">需手工确认</span>
          </div>
        </li>
      </ul>
    </template>
  </div>
</template>

<style scoped>
.dir-health { display: flex; flex-direction: column; gap: 8px; }
.bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.btn {
  font: inherit; font-size: 12px; padding: 5px 12px; border-radius: 6px;
  border: 0.5px solid var(--c-border); background: var(--c-accent); color: #fff; cursor: pointer;
}
.btn.ghost { background: transparent; color: var(--c-text-1); }
.btn.mini { padding: 3px 9px; font-size: 11px; }
.btn:disabled { opacity: 0.5; cursor: default; }
.stage { font-size: 11px; color: var(--c-text-3); }
.err { color: var(--c-danger, #d64545); font-size: 12px; margin: 0; }
.hint { font-size: 12px; color: var(--c-text-2); margin: 0; }
.hint.dim { color: var(--c-text-3); }
.degraded { color: var(--c-warn, #b8860b); }
.note { font-size: 11px; color: var(--c-text-3); margin: 0; }
.issues { list-style: none; margin: 0; padding: 0; max-height: min(320px, 36vh); overflow: auto; min-height: 0; }
.issue { padding: 8px 10px; border: 0.5px solid var(--c-border); border-radius: 8px; display: flex; flex-direction: column; gap: 4px; }
.issue.done { opacity: 0.55; }
.line1, .line2 { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.kind { font-size: 10px; padding: 1px 6px; border-radius: 4px; background: var(--c-surface-2, rgba(127,127,127,.15)); }
.kind.rename { color: #c77b30; }
.kind.move { color: #4a7fd0; }
.kind.merge { color: #8b6fc0; }
.kind.empty { color: var(--c-text-3); }
.path { font-size: 12px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; max-width: 240px; white-space: nowrap; }
.target { font-size: 11px; color: var(--c-accent); }
.conf { margin-left: auto; font-size: 11px; color: var(--c-text-3); }
.reason { font-size: 11px; color: var(--c-text-2); }
.meta { margin-left: auto; font-size: 10px; color: var(--c-text-3); }
.acts { display: flex; gap: 6px; margin-top: 2px; }
.adopted, .ignored-tag, .readonly { font-size: 10px; color: var(--c-text-3); }
</style>
