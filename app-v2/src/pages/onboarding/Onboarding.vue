<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import * as ipc from '../../core/ipc'
import { onScanProgress } from '../../core/events'
import { ERR, humanize, toCmdError } from '../../core/cmdError'
import { useAppStore } from '../../stores/app'
import type { ArchiveInfo, ScanProgress } from '../../core/types'
import type { UnlistenFn } from '@tauri-apps/api/event'

const router = useRouter()
const app = useAppStore()

const step = ref<1 | 2 | 3>(1)
const busy = ref(false)
const err = ref<string | null>(null)

// ---------------- 第一步：源区
const freeGb = ref<number | null>(null)

const canNext = computed(() => app.sourceRoots.length > 0)

async function addRoot() {
  busy.value = true
  err.value = null
  try {
    const p = await ipc.pickDirectory('选择要整理的文件夹')
    if (!p) return
    await ipc.addSourceRoot(p)
    await app.refreshBoot()
    freeGb.value = await ipc.diskFreeGb(app.sourceRoots[0]?.path ?? p).catch(() => null)
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    busy.value = false
  }
}

async function removeRoot(id: number) {
  err.value = null
  try {
    await ipc.removeSourceRoot(id)
    await app.refreshBoot()
  } catch (e) {
    err.value = humanize(toCmdError(e))
  }
}

// ---------------- 第二步：归档区
const archive = ref<ArchiveInfo | null>(null)

async function pickArchive() {
  busy.value = true
  err.value = null
  try {
    const p = await ipc.pickDirectory('选择归档区（建议放在 D 盘）')
    if (!p) return
    archive.value = await ipc.setArchiveRoot(p)
    await app.refreshBoot()
    step.value = 3
  } catch (e) {
    const ce = toCmdError(e)
    // 按 code 分支，不匹配文案（详设 §6.1）
    err.value = ce.code === ERR.ARCHIVE_IN_SOURCE ? '归档区不能放在源区里，换一个位置' : humanize(ce)
  } finally {
    busy.value = false
  }
}

// ---------------- 第三步：扫描
const scanning = ref(false)
const progress = ref<ScanProgress | null>(null)
const scanDone = ref('')
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  if (!app.ready) await app.refreshBoot()
  // 中断过引导的话，归档区可能已经选好了
  if (app.archiveRoot) archive.value = { path: app.archiveRoot, freeGb: null, quarantinePath: '' }
  unlisten = await onScanProgress((p) => {
    progress.value = p
  })
})

onUnmounted(() => unlisten?.())

async function scan() {
  scanning.value = true
  err.value = null
  progress.value = null
  try {
    const r = await ipc.scanAll()
    scanDone.value = `扫了 ${r.scanned} 份，跳过非资料型 ${r.skippedByRule} 份，用时 ${r.elapsedMs}ms`
    await app.refreshBoot()
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    scanning.value = false
  }
}

function toDirs() {
  void router.push('/onboarding/dirs')
}

const progressText = computed(() => {
  const p = progress.value
  if (!p) return '正在读取文件夹…'
  const pct = p.total > 0 ? Math.round((p.processed / p.total) * 100) : 0
  return `已处理 ${p.processed}/${p.total}（${pct}%）`
})
</script>

<template>
  <div class="wrap">
    <header class="hd">
      <h1>先告诉我要整理什么</h1>
      <p class="sub">三步搞定：选要整理的文件夹 → 选归档位置 → 挑一套目录结构。之后随时能改。</p>
      <ol class="steps">
        <li :class="{ on: step === 1, done: step > 1 }">1 源区</li>
        <li :class="{ on: step === 2, done: step > 2 }">2 归档区</li>
        <li :class="{ on: step === 3 }">3 目录方案</li>
      </ol>
    </header>

    <p v-if="err" class="err" role="alert">{{ err }}</p>

    <!-- 第一步 -->
    <section v-if="step === 1" class="panel">
      <div class="row">
        <button class="btn primary" :disabled="busy" @click="addRoot">
          <AppIcon name="plus" :size="14" /> 选择文件夹
        </button>
        <span class="dim">已选 {{ app.sourceRoots.length }} 个源区</span>
        <span v-if="freeGb !== null" class="dim">磁盘剩余 {{ freeGb.toFixed(1) }} GB</span>
      </div>

      <ul class="list">
        <li v-for="s in app.sourceRoots" :key="s.id" class="item">
          <AppIcon name="folder" :size="14" />
          <span class="mono ellipsis">{{ s.path }}</span>
          <button class="link" @click="removeRoot(s.id)">移除</button>
        </li>
        <li v-if="app.sourceRoots.length === 0" class="empty">
          还没选。建议先选「桌面」「下载」「文档」这类最容易堆东西的地方。
        </li>
      </ul>

      <footer class="foot">
        <span class="dim">系统目录（Windows、Program Files 等）自动排除，不会碰。</span>
        <button class="btn primary" :disabled="!canNext" @click="step = 2">下一步</button>
      </footer>
    </section>

    <!-- 第二步 -->
    <section v-else-if="step === 2" class="panel">
      <p class="lead">
        归档区是整理好的文件的落脚点，建议放在空间大的盘（比如 <span class="mono">D:\文档归档</span>）。
      </p>
      <div class="row">
        <button class="btn primary" :disabled="busy" @click="pickArchive">
          <AppIcon name="drive" :size="14" /> 选择归档区
        </button>
        <button class="btn" @click="step = 1">上一步</button>
      </div>
      <p class="hint">
        选好之后会自动建一个隐藏目录 <span class="mono">.filebutler\quarantine</span>，
        搬文件时源文件先进这里，48 小时内都能撤回。
      </p>
    </section>

    <!-- 第三步 -->
    <section v-else class="panel">
      <div class="ok" v-if="archive">
        <AppIcon name="check" :size="14" />
        <span class="mono">{{ archive.path }}</span>
        <span v-if="archive.freeGb !== null" class="dim">{{ archive.freeGb.toFixed(1) }} GB 可用</span>
      </div>

      <p class="lead">先扫一遍现有文件，整理台才知道有哪些东西要归类。只读元信息，不动文件。</p>

      <div class="row">
        <button class="btn primary" :disabled="scanning" @click="scan">
          {{ scanning ? progressText : '开始扫描' }}
        </button>
        <button class="btn" @click="toDirs">去选目录方案</button>
      </div>

      <p v-if="progress?.currentPath" class="hint mono ellipsis">{{ progress.currentPath }}</p>
      <p v-if="scanDone" class="hint ok">{{ scanDone }}</p>
    </section>
  </div>
</template>

<style scoped>
.wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 40px 56px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  background: var(--c-bg);
}

.hd h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.2px;
}
.sub {
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--c-text-2);
}

.steps {
  list-style: none;
  display: flex;
  gap: 18px;
  padding: 0;
  margin: 16px 0 0;
  font-size: 12px;
  color: var(--c-text-3);
}
.steps li.on {
  color: var(--c-primary);
  font-weight: 500;
}
.steps li.done {
  color: var(--c-success-deep);
}

.panel {
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  padding: 20px 22px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 760px;
}

.row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
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
  padding: 7px 10px;
  border-radius: var(--r-item);
  background: var(--c-field);
  font-size: 12px;
}
.empty {
  padding: 10px;
  font-size: 12px;
  color: var(--c-text-2);
  background: var(--c-field);
  border-radius: var(--r-item);
}

.foot {
  display: flex;
  align-items: center;
  gap: 12px;
  border-top: 0.5px solid var(--c-border);
  padding-top: 14px;
}
.foot .btn {
  margin-left: auto;
}

.lead {
  margin: 0;
  font-size: 13px;
  color: var(--c-text-2);
  line-height: 1.7;
}

.hint {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
  line-height: 1.7;
}
.hint.ok {
  color: var(--c-success-deep);
}

.ok {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  font-size: 12px;
}

.err {
  margin: 0;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-danger-bg);
  color: var(--c-danger);
  font-size: 12px;
  max-width: 760px;
}

.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn:disabled {
  opacity: 0.55;
}

.link {
  margin-left: auto;
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 12px;
}

.mono {
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 12px;
  min-width: 0;
}
.dim {
  font-size: 12px;
  color: var(--c-text-2);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
