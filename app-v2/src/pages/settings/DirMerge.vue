<script setup lang="ts">
/**
 * BUG-006 同义目录合并：手动扫描，识别 `2026年9月` ≡ `2026年/9月` 这类同义目录，
 * 逐组展示归一化签名与差异；用户可改选保留目标后采纳，按批次撤销。
 *
 * 纪律：扫描只在用户点按钮时触发（绝不进 refreshBoot）；忽略项不进批次，
 * 并持久化「不再打扰」；采纳走三段式搬移，失败项显式展示。
 */
import { onMounted, onUnmounted, reactive, ref } from 'vue'
import * as ipc from '../../core/ipc'
import { onDirMergeStage } from '../../core/events'
import type {
  DirMergeCandidate,
  DirMergeReport,
  DirMergeStage,
} from '../../core/types'
import type { UnlistenFn } from '@tauri-apps/api/event'

const emit = defineEmits<{ (e: 'changed'): void }>()

const IGNORED_KEY = 'filebutler.dirMerge.ignored.v1'

const busy = ref(false)
const report = ref<DirMergeReport | null>(null)
const stage = ref<DirMergeStage | null>(null)
const err = ref<string | null>(null)
/** 每组当前选中的保留目标（用户可改选） */
const targetOf = reactive<Record<string, number>>({})
const ignored = ref<Set<string>>(loadIgnored())
const adopted = ref<Set<string>>(new Set())
const lastMigrationId = ref<number | null>(null)
let unlisten: UnlistenFn | null = null

function loadIgnored(): Set<string> {
  try {
    const raw = localStorage.getItem(IGNORED_KEY)
    const arr = raw ? JSON.parse(raw) : []
    return new Set(Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string') : [])
  } catch {
    return new Set()
  }
}

function persistIgnored() {
  try {
    localStorage.setItem(IGNORED_KEY, JSON.stringify([...ignored.value]))
  } catch {
    // 存储不可用（隐私模式）时仅本次会话生效，不影响扫描与采纳
  }
}

onMounted(async () => {
  unlisten = await onDirMergeStage((p) => {
    stage.value = p
  })
})
onUnmounted(() => unlisten?.())

const STAGE_LABEL: Record<string, string> = {
  scan: '正在扫描目录',
  rule: '规则初筛中',
  model: '模型复核中',
  copy: '正在复制',
  verify: '正在校验',
  quarantine: '源文件隔离中',
  done: '完成',
}

const stageText = () => {
  if (!stage.value) return ''
  return stage.value.note?.trim() || STAGE_LABEL[stage.value.stage] || stage.value.stage
}

/** 忽略过的组不再进列表，也不进执行批次 */
const visible = () => (report.value?.candidates ?? []).filter((c) => !ignored.value.has(c.id))

async function scan() {
  busy.value = true
  err.value = null
  report.value = null
  stage.value = null
  adopted.value = new Set()
  try {
    const r = await ipc.dirMergeScan()
    // 被忽略的组不进列表：用户已经明确说过「不要再提醒」
    report.value = { ...r, candidates: r.candidates.filter((c) => !ignored.value.has(c.id)) }
    for (const c of report.value.candidates) targetOf[c.id] = c.suggestedTargetId
  } catch (e: unknown) {
    err.value = e instanceof Error ? e.message : '扫描失败，请稍后再试'
  } finally {
    busy.value = false
  }
}

function ignore(c: DirMergeCandidate) {
  const next = new Set(ignored.value)
  next.add(c.id)
  ignored.value = next
  persistIgnored()
}

async function adopt(c: DirMergeCandidate) {
  if (busy.value) return
  const chosen = Number(targetOf[c.id] ?? c.suggestedTargetId)
  if (!c.dirIds.includes(chosen)) {
    err.value = '请选择一个有效的保留目录'
    return
  }
  busy.value = true
  err.value = null
  try {
    const ack = await ipc.applyDirMerge({
      candidateId: c.id,
      sourceDirIds: c.dirIds.filter((id) => id !== chosen),
      targetDirId: chosen,
    })
    lastMigrationId.value = ack.migrationId
    adopted.value = new Set([...adopted.value, c.id])
    if (ack.failed.length) {
      err.value = `有 ${ack.failed.length} 份未能合并：${ack.failed.map((f) => f.name).join('、')}`
    }
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
    await ipc.undoDirMerge(lastMigrationId.value)
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
  <div class="dir-merge">
    <div class="bar">
      <button class="btn primary" :disabled="busy" @click="scan">同义目录扫描</button>
      <button class="btn ghost" :disabled="busy || lastMigrationId === null" @click="undo">
        撤销上次合并
      </button>
      <span v-if="stage" class="stage">{{ stageText() }}</span>
    </div>

    <p v-if="err" class="err">{{ err }}</p>

    <template v-if="report">
      <p class="hint">
        共 {{ visible().length }} 组建议
        <span v-if="!report.modelReviewed" class="degraded">（未经模型复核：仅规则初筛）</span>
      </p>
      <p v-if="report.note" class="note">{{ report.note }}</p>
      <p v-if="ignored.size" class="hint dim">已忽略 {{ ignored.size }} 组，不再自动提醒。</p>
      <p v-if="visible().length === 0" class="hint dim">没有发现可合并的同义目录。</p>

      <ul v-else class="groups">
        <li v-for="c in visible()" :key="c.id" class="group" :class="{ done: adopted.has(c.id) }">
          <div class="line1">
            <span class="paths">{{ c.dirPaths.join('  ⇄  ') }}</span>
            <span class="conf">{{ Math.round(c.confidence * 100) }}%</span>
          </div>
          <div class="line2">
            <span class="reason">{{ c.reason }}</span>
            <span class="meta">{{ c.fileCount }} 份文件 · {{ c.source === 'model' ? '模型确认' : '规则' }}</span>
          </div>
          <div class="acts">
            <label class="pick">
              保留
              <select v-model="targetOf[c.id]" :disabled="busy || adopted.has(c.id)">
                <option v-for="(p, i) in c.dirPaths" :key="c.dirIds[i]" :value="c.dirIds[i]">
                  {{ p }}
                </option>
              </select>
            </label>
            <template v-if="!adopted.has(c.id)">
              <button class="btn mini primary" :disabled="busy" @click="adopt(c)">采纳</button>
              <button class="btn mini ghost" :disabled="busy" @click="ignore(c)">忽略</button>
            </template>
            <span v-else class="adopted">已合并</span>
          </div>
        </li>
      </ul>
    </template>
  </div>
</template>

<style scoped>
.dir-merge { display: flex; flex-direction: column; gap: 8px; }
.bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.btn.ghost { background: var(--c-field); color: var(--c-text); box-shadow: none; }
.btn.mini { padding: 3px 9px; font-size: 11px; }
.btn:disabled { opacity: 0.5; cursor: default; }
.stage { font-size: 11px; color: var(--c-text-3); }
.err { color: var(--c-danger); font-size: 12px; margin: 0; }
.hint { font-size: 12px; color: var(--c-text-2); margin: 0; }
.hint.dim { color: var(--c-text-3); }
.degraded { color: var(--c-warning-deep); }
.note { font-size: 11px; color: var(--c-text-3); margin: 0; }
.groups { list-style: none; margin: 0; padding: 0; max-height: min(320px, 36vh); overflow: auto; min-height: 0; }
.group { padding: 8px 10px; border: 0.5px solid var(--c-border); border-radius: 8px; display: flex; flex-direction: column; gap: 4px; }
.group.done { opacity: 0.55; }
.line1, .line2 { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.paths { font-size: 12px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; max-width: 300px; white-space: nowrap; }
.conf { margin-left: auto; font-size: 11px; color: var(--c-text-3); }
.reason { font-size: 11px; color: var(--c-text-2); }
.meta { margin-left: auto; font-size: 10px; color: var(--c-text-3); }
.acts { display: flex; align-items: center; gap: 6px; margin-top: 2px; flex-wrap: wrap; }
.pick { font-size: 11px; color: var(--c-text-2); display: inline-flex; align-items: center; gap: 4px; }
.pick select { font-size: 11px; max-width: 240px; background: var(--c-field); color: var(--c-text); border: 0.5px solid var(--c-border); border-radius: 6px; padding: 2px 4px; }
.adopted { font-size: 10px; color: var(--c-text-3); }
</style>
