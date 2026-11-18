/**
 * 待迁移页的状态（T7.6）
 *
 * 两条铁律：
 * 1. **默认全选** —— 这些是用户一份份确认过的，默认意图就是「都搬」；
 * 2. **有「标注后被改」的文件就不许搬** —— 拦截条件由状态算，不由按钮自己判断，
 *    否则总有一处漏掉。
 */

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import * as ipc from '../core/ipc'
import { toCmdError } from '../core/cmdError'
import type {
  FileRow,
  MigrateProgress,
  MigrationPreview,
  MigrationResult,
  MigrationSummary,
  UndoReport,
} from '../core/types'

export const useMigrationStore = defineStore('migration', () => {
  const preview = ref<MigrationPreview | null>(null)
  const latest = ref<MigrationSummary | null>(null)
  const selected = ref<Set<number>>(new Set())
  const loading = ref(false)
  const busy = ref(false)
  const error = ref<string | null>(null)
  const result = ref<MigrationResult | null>(null)
  const undoReport = ref<UndoReport | null>(null)
  const progress = ref<MigrateProgress | null>(null)

  const groups = computed(() => preview.value?.groups ?? [])
  const allFiles = computed<FileRow[]>(() => groups.value.flatMap((g) => g.files))
  const changed = computed(() => preview.value?.changedSinceLabel ?? [])
  const conflicts = computed(() => preview.value?.nameConflicts ?? [])
  const enoughSpace = computed(() => preview.value?.enoughSpace ?? true)
  const freeGb = computed(() => preview.value?.freeGb ?? null)
  const totalFiles = computed(() => preview.value?.totalFiles ?? 0)
  const totalBytes = computed(() => preview.value?.totalBytes ?? 0)
  const empty = computed(() => !loading.value && totalFiles.value === 0)

  const selectedFiles = computed(() => allFiles.value.filter((f) => selected.value.has(f.id)))
  const selectedCount = computed(() => selectedFiles.value.length)
  const selectedBytes = computed(() =>
    selectedFiles.value.reduce((sum, f) => sum + Math.max(0, f.size), 0),
  )

  /** 唯一允许放行的条件（按钮只读它） */
  const canApply = computed(
    () => !busy.value && selectedCount.value > 0 && changed.value.length === 0 && enoughSpace.value,
  )
  const canUndo = computed(
    () => !!latest.value && (latest.value.status === 'done' || latest.value.status === 'partial'),
  )
  const undoneAlready = computed(() => latest.value?.status === 'undone')

  function isSelected(fileId: number) {
    return selected.value.has(fileId)
  }

  function groupSelected(dirId: number) {
    const g = groups.value.find((x) => x.dirId === dirId)
    if (!g || g.files.length === 0) return false
    return g.files.every((f) => selected.value.has(f.id))
  }

  function toggle(fileId: number) {
    const next = new Set(selected.value)
    if (next.has(fileId)) next.delete(fileId)
    else next.add(fileId)
    selected.value = next
  }

  function toggleGroup(dirId: number) {
    const g = groups.value.find((x) => x.dirId === dirId)
    if (!g) return
    const on = !groupSelected(dirId)
    const next = new Set(selected.value)
    for (const f of g.files) {
      if (on) next.add(f.id)
      else next.delete(f.id)
    }
    selected.value = next
  }

  function selectAll(on: boolean) {
    selected.value = on ? new Set(allFiles.value.map((f) => f.id)) : new Set()
  }

  async function load() {
    loading.value = true
    // 刷新一次就把上一次的进度条抹掉：留着会让人以为还在搬
    progress.value = null
    try {
      const [p, l] = await Promise.all([ipc.migrationPreview(), ipc.latestMigration()])
      preview.value = p
      latest.value = l
      selected.value = new Set(p.groups.flatMap((g) => g.files.map((f) => f.id)))
      error.value = null
      return p
    } catch (e) {
      error.value = toCmdError(e).message
      preview.value = null
      return null
    } finally {
      loading.value = false
    }
  }

  async function apply() {
    if (!canApply.value) return null
    busy.value = true
    progress.value = null
    undoReport.value = null
    try {
      const exclude = allFiles.value.filter((f) => !selected.value.has(f.id)).map((f) => f.id)
      const r = await ipc.applyMigration(exclude)
      result.value = r
      error.value = null
      await load()
      return r
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    } finally {
      busy.value = false
      progress.value = null
    }
  }

  async function undo() {
    // 刚搬完就撤：撤的是这一次；重启之后才靠 latest（数据库里最后一条）
    const id = result.value?.migrationId ?? latest.value?.id
    if (!id) return null
    busy.value = true
    progress.value = null
    try {
      const r = await ipc.undoMigration(id)
      undoReport.value = r
      // 撤完了就别再挂着「已迁移 N 份」的结果条，两个横幅同时出现会让人以为没撤成
      result.value = null
      error.value = null
      await load()
      return r
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    } finally {
      busy.value = false
      progress.value = null
    }
  }

  /** 标注后被改：退回整理台重新确认（后端会顺手刷新 size/mtime） */
  async function requeue(fileId: number) {
    try {
      await ipc.requeueConfirmed(fileId)
      await load()
      return true
    } catch (e) {
      error.value = toCmdError(e).message
      return false
    }
  }

  function setProgress(p: MigrateProgress | null) {
    progress.value = p
  }

  function clearResult() {
    result.value = null
  }

  return {
    preview,
    latest,
    selected,
    loading,
    busy,
    error,
    result,
    undoReport,
    progress,
    groups,
    allFiles,
    changed,
    conflicts,
    enoughSpace,
    freeGb,
    totalFiles,
    totalBytes,
    empty,
    selectedCount,
    selectedBytes,
    canApply,
    canUndo,
    undoneAlready,
    isSelected,
    groupSelected,
    toggle,
    toggleGroup,
    selectAll,
    load,
    apply,
    undo,
    requeue,
    setProgress,
    clearResult,
  }
})
