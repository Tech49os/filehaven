<script setup lang="ts">
/**
 * 待迁移（T7.6）
 *
 * 一屏说清三件事：要搬什么（分组 + 份数 + 字节）、有什么风险（同名冲突 / 标注后被改）、
 * 能不能按下去（空间够不够、有没有未处理的告警）。
 *
 * 拦截逻辑不写在按钮的 `:disabled` 里各写一遍，而是统一读 store 的 `canApply` ——
 * 否则「取消勾选」「重新确认」「空间不足」三条路各漏一次。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import EmptyState from '../../components/EmptyState.vue'
import { formatSize } from '../../core/format'
import { statusLabel } from '../../core/format'
import { onMigrateProgress, onUndoProgress } from '../../core/events'
import { useAppStore } from '../../stores/app'
import { useMigrationStore } from '../../stores/migration'
import type { UnlistenFn } from '@tauri-apps/api/event'

const router = useRouter()
const app = useAppStore()
const store = useMigrationStore()

const stageText = computed(() => {
  const s = store.progress?.stage
  if (s === 'copy') return '复制中'
  if (s === 'verify') return '校验中'
  if (s === 'quarantine') return '移入隔离区'
  return ''
})

const changedWhy = (why: string) =>
  why === 'size' ? '体积变了' : why === 'mtime' ? '修改时间变了' : '文件不见了'

const conflictOf = (fileId: number) =>
  store.conflicts.find((c) => c.fileId === fileId)?.targetName ?? null

let unlisten: UnlistenFn[] = []

onMounted(async () => {
  await store.load()
  void app.refreshBoot()
  unlisten = [await onMigrateProgress((p) => store.setProgress(p)), await onUndoProgress((p) => store.setProgress(p))]
})

onUnmounted(() => {
  unlisten.forEach((fn) => fn())
})

async function onApply() {
  const r = await store.apply()
  if (r) void app.refreshBoot()
}

async function onUndo() {
  const r = await store.undo()
  if (r) void app.refreshBoot()
}

async function onRequeue(fileId: number) {
  await store.requeue(fileId)
  void app.refreshBoot()
}

const busy = ref(false)

async function reload() {
  busy.value = true
  try {
    await store.load()
    await app.refreshBoot()
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="mg">
    <header class="top">
      <div class="title">待迁移</div>
      <div v-if="!store.empty" class="sum">
        {{ store.totalFiles }} 份 · {{ formatSize(store.totalBytes) }}
      </div>
      <div class="acts">
        <button class="btn" :disabled="busy || store.busy" @click="reload">刷新</button>
      </div>
    </header>

    <p v-if="store.error" class="err" role="alert">{{ store.error }}</p>

    <!-- 标注后被改：红色拦截，必须先处理 -->
    <section v-if="store.changed.length > 0" class="warn">
      <div class="wtitle">
        <AppIcon name="shield" :size="14" />
        有 {{ store.changed.length }} 份在标注之后被改过，先处理再搬
      </div>
      <ul class="wlist">
        <li v-for="c in store.changed" :key="c.fileId" class="wrow">
          <span class="wname ellipsis" :title="c.name">{{ c.name }}</span>
          <span class="wwhy">{{ changedWhy(c.why) }}</span>
          <button class="btn small" @click="onRequeue(c.fileId)">重新确认</button>
        </li>
      </ul>
      <p class="whint">
        重新确认会把它退回整理台，并按现在的文件内容重新归档 —— 我们不会拿旧的判断去搬新的内容。
      </p>
    </section>

    <!-- 上次的结果 / 撤销入口 -->
    <section v-if="store.result" class="result ok">
      <AppIcon name="check" :size="14" />
      <span>已迁移 {{ store.result.copied }} 份</span>
      <span v-if="store.result.failed.length" class="bad">
        · 失败 {{ store.result.failed.length }} 份
      </span>
      <span class="dim">归档副本已逐个校验，源文件在隔离区里，随时可撤回</span>
      <button class="btn small" :disabled="store.busy" @click="onUndo">撤销这次迁移</button>
      <button class="btn small" @click="router.push('/browser')">去浏览</button>
    </section>
    <section v-else-if="store.undoReport" class="result">
      <AppIcon name="undo" :size="14" />
      <span>已还原 {{ store.undoReport.restored }} 份</span>
      <span v-if="store.undoReport.failed.length" class="bad">
        · 失败 {{ store.undoReport.failed.length }} 份
      </span>
      <button class="btn small" @click="router.push('/workspace')">回整理台</button>
    </section>
    <ul v-if="store.result?.failed.length" class="flist">
      <li v-for="f in store.result.failed" :key="f.fileId">
        <span class="ellipsis">{{ f.name }}</span>
        <span class="why">{{ f.reason }}</span>
      </li>
    </ul>

    <!-- 分组清单 -->
    <div v-if="!store.empty" class="groups">
      <section v-for="g in store.groups" :key="g.dirId" class="group">
        <header class="ghead">
          <input
            type="checkbox"
            :checked="store.groupSelected(g.dirId)"
            :aria-label="`选择 ${g.relPath} 整个目录`"
            @change="store.toggleGroup(g.dirId)"
          />
          <AppIcon name="folder" :size="14" />
          <span class="gname ellipsis" :title="g.relPath">{{ g.relPath }}</span>
          <span v-if="g.isNew" class="tag">新建目录</span>
          <span class="gmeta">{{ g.files.length }} 份 · {{ formatSize(g.bytes) }}</span>
        </header>
        <ul class="flist-body">
          <li v-for="f in g.files" :key="f.id" class="frow">
            <input
              type="checkbox"
              :checked="store.isSelected(f.id)"
              :aria-label="`选择 ${f.name}`"
              @change="store.toggle(f.id)"
            />
            <span class="fname ellipsis" :title="f.name">{{ f.name }}</span>
            <span class="fsize">{{ formatSize(f.size) }}</span>
            <span class="fst">{{ statusLabel(f.status) }}</span>
            <span v-if="conflictOf(f.id)" class="conflict">
              同名，将存为 {{ conflictOf(f.id) }}
            </span>
          </li>
        </ul>
      </section>
    </div>

    <EmptyState
      v-else-if="!store.loading"
      icon="check"
      title="没有待迁移的文件"
      desc="在整理台确认过的文件会汇总到这里：先看分组、再看告警，最后一眼就能搬。"
    >
      <button class="btn primary" @click="router.push('/workspace')">去整理台</button>
    </EmptyState>

    <!-- 底部固定条 -->
    <footer v-if="!store.empty" class="foot">
      <span class="count">已选 {{ store.selectedCount }} 份 · {{ formatSize(store.selectedBytes) }}</span>
      <span v-if="store.progress" class="prog">
        {{ store.progress.done }}/{{ store.progress.total }} · {{ stageText }} ·
        <span class="ellipsis pname">{{ store.progress.name }}</span>
      </span>
      <span v-else-if="!store.enoughSpace" class="hint bad">
        归档盘只剩 {{ store.freeGb?.toFixed(1) ?? '?' }}GB，空间不够
      </span>
      <span v-else-if="store.changed.length > 0" class="hint">
        先处理上面 {{ store.changed.length }} 份被改过的文件
      </span>
      <button class="btn primary" :disabled="!store.canApply" @click="onApply">
        {{ store.busy ? '正在迁移…' : `应用迁移（${store.selectedCount} 份）` }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.mg {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.top {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 0 0 auto;
}
.title {
  font-size: 15px;
  font-weight: 600;
}
.sum {
  font-size: 12px;
  color: var(--c-text-2);
}
.acts {
  margin-left: auto;
  display: flex;
  gap: 8px;
}

.err {
  margin: 0;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-danger-bg);
  color: var(--c-danger);
  font-size: 12px;
}

.warn {
  border: 0.5px solid rgba(220, 60, 60, 0.35);
  background: var(--c-danger-bg);
  border-radius: var(--r-card);
  padding: 10px 14px;
}
.wtitle {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  font-weight: 500;
  color: var(--c-danger);
}
.wlist {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.wrow {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 10px;
  font-size: 12px;
}
.wname {
  min-width: 0;
}
.wwhy {
  color: var(--c-danger);
  font-size: 11px;
}
.whint {
  margin: 8px 0 0;
  font-size: 11px;
  line-height: 1.6;
  color: var(--c-text-2);
}

.result {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 14px;
  border-radius: var(--r-banner);
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  font-size: 12px;
}
.result.ok {
  border-color: rgba(40, 200, 64, 0.35);
  color: var(--c-success-deep);
}
.result .dim {
  color: var(--c-text-2);
}
.result .btn:first-of-type {
  margin-left: auto;
}
.bad {
  color: var(--c-danger);
}

.flist {
  list-style: none;
  margin: 0;
  padding: 6px 12px;
  border-radius: var(--r-card);
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  font-size: 11px;
  color: var(--c-text-2);
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.flist li {
  display: flex;
  gap: 8px;
  min-width: 0;
}
.flist .why {
  color: var(--c-danger);
}

.groups {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
}
.group {
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  overflow: hidden;
}
.ghead {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 9px 14px;
  border-bottom: 0.5px solid var(--c-border);
  font-size: 13px;
}
.gname {
  min-width: 0;
  font-weight: 500;
}
.tag {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 5px;
  background: var(--c-primary-bg);
  color: var(--c-primary);
}
.gmeta {
  margin-left: auto;
  font-size: 11px;
  color: var(--c-text-3);
  white-space: nowrap;
}

.flist-body {
  list-style: none;
  margin: 0;
  padding: 4px 8px 6px;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.frow {
  display: grid;
  grid-template-columns: 14px minmax(0, 1fr) 54px 48px;
  align-items: center;
  column-gap: 8px;
  padding: 4px 6px;
  border-radius: var(--r-item);
  font-size: 12px;
}
.frow:hover {
  background: var(--c-hover);
}
.fname {
  min-width: 0;
}
.fsize,
.fst {
  font-size: 11px;
  color: var(--c-text-3);
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.conflict {
  grid-column: 2 / -1;
  font-size: 10px;
  color: var(--c-warn, #b26a00);
}

.foot {
  position: sticky;
  bottom: 0;
  margin-top: auto;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-radius: var(--r-banner);
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.08);
  font-size: 12px;
}
.count {
  font-weight: 500;
}
.prog {
  color: var(--c-text-2);
  font-size: 11px;
  display: inline-flex;
  gap: 4px;
  min-width: 0;
}
.pname {
  max-width: 180px;
}
.hint {
  color: var(--c-text-2);
  font-size: 11px;
}
.foot .btn.primary {
  margin-left: auto;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: 0;
  border-radius: var(--r-btn);
  padding: 6px 12px;
  font-size: 12px;
  background: var(--c-field);
  color: var(--c-text);
  font-family: var(--font);
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn.small {
  padding: 4px 10px;
  font-size: 11px;
}
.btn:disabled {
  opacity: 0.55;
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
