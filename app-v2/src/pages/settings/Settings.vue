<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import Card from '../../components/Card.vue'
import AppIcon from '../../components/AppIcon.vue'
import DirTree from '../../components/DirTree.vue'
import NewDirInput from '../../components/NewDirInput.vue'
import { useAppStore } from '../../stores/app'
import { useDirsStore } from '../../stores/dirs'
import * as ipc from '../../core/ipc'
import { onScanProgress } from '../../core/events'
import { humanize, toCmdError } from '../../core/cmdError'
import type { ScanProgress, ScanReport } from '../../core/types'
import type { UnlistenFn } from '@tauri-apps/api/event'

const app = useAppStore()
const dirs = useDirsStore()

const collapsed = ref<Set<number>>(new Set())
const adding = ref(false)

function toggleDir(id: number) {
  const next = new Set(collapsed.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  collapsed.value = next
}

async function onCreateDir(input: string) {
  adding.value = false
  try {
    await dirs.createDir(input)
  } catch (e) {
    err.value = humanize(toCmdError(e))
  }
}
const scanning = ref(false)
const report = ref<ScanReport | null>(null)
const progress = ref<ScanProgress | null>(null)
const freeGb = ref<number | null>(null)
const err = ref<string | null>(null)
const addedHint = ref('')
const picking = ref(false)
const probing = ref(false)

/** 模型探活只在这里和启动时真跑（别把它挂回 refreshBoot 的默认路径） */
async function probeModel() {
  probing.value = true
  try {
    await app.refreshBoot({ probe: true })
    if (!app.modelStatus.reachable) err.value = '模型没连上，先启动 Ollama 再点一次'
    else err.value = null
  } finally {
    probing.value = false
  }
}

let unlisten: UnlistenFn | null = null

onMounted(async () => {
  // 直接进 /settings（刷新、深链）时 store 还是空的，这里补一次，
  // 否则源区列表会显示「还没有源区」而实际库里有 —— 用户会以为被清了
  if (!app.ready) await app.refreshBoot()
  if (app.archiveRoot) freeGb.value = await ipc.diskFreeGb(app.archiveRoot).catch(() => null)
  await dirs.load()
  unlisten = await onScanProgress((p) => {
    progress.value = p
  })
})

onUnmounted(() => unlisten?.())

const scanLine = computed(() => {
  const r = report.value
  if (!r) return ''
  return `扫了 ${r.scanned} 份 · 新增 ${r.inserted} · 更新 ${r.updated} · 跳过非资料型 ${r.skippedByRule} · 用时 ${r.elapsedMs}ms`
})

const progressLine = computed(() => {
  const p = progress.value
  if (!p || !scanning.value) return ''
  if (p.total <= 0) return '正在读取文件夹…'
  return `已处理 ${p.processed}/${p.total}（${Math.round((p.processed / p.total) * 100)}%）`
})

async function rescan() {
  scanning.value = true
  err.value = null
  progress.value = null
  try {
    report.value = await ipc.scanAll()
    await app.refreshBoot()
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    scanning.value = false
  }
}

async function addRoot() {
  picking.value = true
  err.value = null
  addedHint.value = ''
  try {
    const p = await ipc.pickDirectory('选择要整理的文件夹', app.sourceRoots[0]?.path)
    if (!p) return
    await ipc.addSourceRoot(p)
    await app.refreshBoot()
    addedHint.value = `已添加 ${p}，点「重新扫描」把里面的文件读进来`
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    picking.value = false
  }
}

async function removeRoot(id: number) {
  try {
    await ipc.removeSourceRoot(id)
    await app.refreshBoot()
    addedHint.value = ''
  } catch (e) {
    err.value = humanize(toCmdError(e))
  }
}
</script>

<template>
  <div class="grid">
    <Card title="归档区">
      <div class="row">
        <AppIcon name="drive" :size="16" />
        <span class="mono ellipsis">{{ app.archiveRoot ?? '未设置' }}</span>
        <span v-if="freeGb !== null" class="dim">{{ freeGb.toFixed(1) }} GB 可用</span>
      </div>
      <p class="hint">
        隔离区位于 <span class="mono">{{ app.archiveRoot }}\.filebutler\quarantine</span>，
        迁移时源文件先进这里，随时能撤回。
      </p>
    </Card>

    <Card title="源区">
      <template #action>
        <div class="acts">
          <button class="btn ghost" :disabled="picking || scanning" @click="addRoot">
            <AppIcon name="plus" :size="12" /> 添加源区
          </button>
          <button class="btn" :disabled="scanning || picking" @click="rescan">
            {{ scanning ? '扫描中…' : '重新扫描' }}
          </button>
        </div>
      </template>
      <ul class="list">
        <li v-for="s in app.sourceRoots" :key="s.id" class="item">
          <AppIcon name="folder" :size="14" />
          <span class="mono ellipsis">{{ s.path }}</span>
          <button class="link" @click="removeRoot(s.id)">移除</button>
        </li>
        <li v-if="app.sourceRoots.length === 0" class="empty">
          还没有源区。
          <button class="link inline" :disabled="picking" @click="addRoot">选一个文件夹</button>
        </li>
      </ul>
      <p v-if="progressLine" class="hint mono">{{ progressLine }}</p>
      <p v-if="addedHint" class="hint ok">{{ addedHint }}</p>
      <p v-if="scanLine" class="hint ok">{{ scanLine }}</p>
      <p v-if="err" class="hint bad">{{ err }}</p>
    </Card>

    <Card title="目录结构">
      <template #action>
        <button class="btn" @click="adding = true">新建目录</button>
      </template>
      <DirTree
        v-if="dirs.tree.length > 0"
        :nodes="dirs.tree"
        :selected-id="dirs.selectedId"
        :collapsed="collapsed"
        @select="dirs.select($event)"
        @toggle="toggleDir"
      />
      <p v-else class="hint">还没有目录。整理台里按 N，或这里点「新建目录」。</p>
      <div v-if="adding" class="newdir">
        <NewDirInput @confirm="onCreateDir" @cancel="adding = false" />
      </div>
    </Card>

    <Card title="本地模型">
      <template #action>
        <button class="btn ghost" :disabled="probing" @click="probeModel">
          {{ probing ? '检测中…' : '重新检测' }}
        </button>
      </template>
      <div class="row">
        <i class="dot" :class="{ on: app.modelStatus.reachable }" />
        <span>{{ app.modelStatus.reachable ? '已连接' : '未连接' }}</span>
        <span class="dim mono">{{ app.modelStatus.model ?? '—' }}</span>
      </div>
      <p class="hint">
        只有「让 AI 建议目录结构」用模型；模型断开时整理台照常可用，只是目录方案要自己写。
      </p>
    </Card>
  </div>
</template>

<style scoped>
.grid {
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow: auto;
  min-height: 0;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--r-item);
  background: var(--c-field);
  font-size: 12px;
}

.newdir {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 0.5px solid var(--c-border);
}

.empty {
  padding: 8px;
  font-size: 12px;
  color: var(--c-text-3);
}

.hint {
  margin: 10px 0 0;
  font-size: 12px;
  color: var(--c-text-2);
  line-height: 1.7;
}
.hint.ok {
  color: var(--c-success-deep);
}
.hint.bad {
  color: var(--c-danger);
}

.mono {
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 12px;
  min-width: 0;
}

.dim {
  color: var(--c-text-2);
  font-size: 12px;
  margin-left: auto;
}

.btn {
  border: 0;
  border-radius: var(--r-btn);
  padding: 5px 12px;
  font-size: 12px;
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
  display: inline-flex;
  align-items: center;
  gap: 4px;
  transition: background var(--dur-fast) ease, transform var(--dur-fast) ease,
    box-shadow var(--dur-fast) ease;
}
.btn:active:not(:disabled) {
  transform: scale(0.96);
}
.btn.ghost {
  background: var(--c-field);
  color: var(--c-text);
  box-shadow: none;
}
.btn.ghost:hover:not(:disabled) {
  background: var(--c-hover);
}
.btn:disabled {
  opacity: 0.6;
}

.acts {
  display: flex;
  align-items: center;
  gap: 8px;
}

.link {
  margin-left: auto;
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 12px;
  padding: 0 2px;
}
.link.inline {
  margin-left: 2px;
  padding: 0;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--c-text-3);
}
.dot.on {
  background: var(--c-success);
  box-shadow: 0 0 0 3px var(--c-success-bg);
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
