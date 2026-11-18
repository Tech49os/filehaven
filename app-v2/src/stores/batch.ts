/**
 * 整理台的批次状态（T6.9 / T6.14）
 *
 * 一条铁律：**判定成功就立刻改本地 item 的状态，不重拉整批**。
 * 重拉会让焦点跳动、卡片闪、已确认列表重排，用户会以为点错了。
 */

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import * as ipc from '../core/ipc'
import { toCmdError } from '../core/cmdError'
import { nextUnconfirmed, popUndo, pushUndo } from '../core/focus'
import type {
  BatchItem,
  BatchView,
  Candidate,
  CloseAck,
  DecideAck,
  Decision,
  DecideOrigin,
} from '../core/types'

export const useBatchStore = defineStore('batch', () => {
  const view = ref<BatchView | null>(null)
  const index = ref(0)
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** 已处理但有别于 confirmed 的（暂缓 / 先放着），避免焦点又落回它们 */
  const resolved = ref<Set<number>>(new Set())
  const undoStack = ref<number[]>([])
  const finished = ref<CloseAck | null>(null)
  /** 新建目录输入框是否打开（决定键盘谁说话） */
  const inputOpen = ref(false)
  const lastAck = ref<DecideAck | null>(null)

  const items = computed<BatchItem[]>(() => view.value?.items ?? [])
  const current = computed<BatchItem | null>(() => items.value[index.value] ?? null)
  const total = computed(() => items.value.length)
  const batchId = computed(() => view.value?.batchId ?? 0)
  const seq = computed(() => view.value?.seq ?? 0)
  const dirCount = computed(() => view.value?.dirCount ?? 0)
  const remaining = computed(() => view.value?.remainingInPool ?? 0)
  const deferredBadge = computed(() => new Set(view.value?.deferredBadge ?? []))

  const confirmedCount = computed(
    () => items.value.filter((i) => i.file.status === 'confirmed').length,
  )
  const confirmedList = computed(() => items.value.filter((i) => i.file.status === 'confirmed'))

  /** 每份都处理过了（确认 / 暂缓 / 先放着都算处理） */
  const allDone = computed(
    () =>
      items.value.length > 0 &&
      items.value.every(
        (i) => i.file.status === 'confirmed' || resolved.value.has(i.file.id),
      ),
  )

  function isResolved(it: BatchItem) {
    return it.file.status === 'confirmed' || resolved.value.has(it.file.id)
  }

  /** 找下一个没处理的位置；找不到就停在原地 */
  function nextIndexFrom(from: number): number {
    const n = items.value.length
    for (let step = 1; step <= n; step += 1) {
      const i = (from + step) % n
      if (!isResolved(items.value[i])) return i
    }
    return from
  }

  async function load(size = 12) {
    loading.value = true
    error.value = null
    finished.value = null
    resolved.value = new Set()
    undoStack.value = []
    try {
      view.value = await ipc.nextBatch(size)
      // 开批时从第一个未确认项开始（暂缓回来的排最后）
      index.value = Math.max(0, nextUnconfirmed(items.value, 0))
      if (index.value < 0) index.value = 0
      return view.value
    } catch (e) {
      error.value = toCmdError(e).message
      view.value = null
      return null
    } finally {
      loading.value = false
    }
  }

  /** 判定落库的统一入口：三个按钮、备选、新建目录都走这里 */
  async function apply(
    dirId: number | null,
    decision: Decision,
    origin: DecideOrigin,
    cand: Candidate | null,
    note?: string,
  ) {
    const it = current.value
    if (!it || !view.value) return null
    try {
      const ack = await ipc.decide({
        fileId: it.file.id,
        batchId: view.value.batchId,
        dirId,
        decision,
        origin,
        confidence: cand?.score ?? null,
        reason: cand?.reason ?? null,
        note,
      })
      it.file.status = ack.status
      lastAck.value = ack
      if (decision === 'accepted' || decision === 'changed') {
        undoStack.value = pushUndo(undoStack.value, it.file.id)
      } else {
        resolved.value = new Set(resolved.value).add(it.file.id)
      }
      error.value = null
      index.value = nextIndexFrom(index.value)
      return ack
    } catch (e) {
      // 失败要吵出来：静默失败的判定等于欺骗用户
      error.value = toCmdError(e).message
      return null
    }
  }

  /** 回车 / 点主推：[回车] ① */
  async function accept() {
    const it = current.value
    if (!it) return null
    const c = it.candidates[0]
    if (!c) {
      // needsYou：没有候选，只能新建目录或先放着
      error.value = '这份没有推荐目录，按 N 新建一个，或按 S 暂缓'
      return null
    }
    return apply(c.dirId, 'accepted', 'recommended', c)
  }

  /** 数字键选备选 */
  async function pick(n: 1 | 2 | 3) {
    const it = current.value
    if (!it) return null
    const c = it.candidates[n - 1]
    if (!c) return null
    if (n === 1) return apply(c.dirId, 'accepted', 'recommended', c)
    return apply(c.dirId, 'changed', 'user_picked', c)
  }

  /** 新建目录后落判定：origin=new_dir */
  async function newDirConfirm(input: string) {
    const it = current.value
    if (!it) return null
    inputOpen.value = false
    try {
      const d = await ipc.createDir(input)
      return await apply(d.id, 'changed', 'new_dir', null)
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    }
  }

  async function defer() {
    return apply(null, 'deferred', 'user_changed', null)
  }

  async function park(note?: string) {
    return apply(null, 'parked', 'user_changed', null, note)
  }

  /** Ctrl+Z：撤销最近一次确认并回填 */
  async function undo() {
    if (!view.value || undoStack.value.length === 0) return
    const { fileId, rest } = popUndo(undoStack.value)
    if (fileId === null) return
    try {
      const back = await ipc.undoDecide(view.value.batchId)
      undoStack.value = rest
      if (!back) return
      const i = items.value.findIndex((x) => x.file.id === back.file.id)
      if (i >= 0) {
        items.value[i] = { ...back }
        index.value = i
      }
      error.value = null
    } catch (e) {
      error.value = toCmdError(e).message
    }
  }

  function move(d: 1 | -1) {
    inputOpen.value = false
    const n = items.value.length
    if (n === 0) return
    index.value = Math.min(n - 1, Math.max(0, index.value + d))
  }

  function focus(i: number) {
    if (i >= 0 && i < items.value.length) index.value = i
  }

  function openInput() {
    inputOpen.value = true
  }

  function closeInput() {
    inputOpen.value = false
  }

  /** 结束本批：未确认的回池，拿到剩余池量给完成提示条用 */
  async function finish() {
    if (!view.value) return null
    try {
      const ack = await ipc.closeBatch(view.value.batchId)
      finished.value = ack
      return ack
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    }
  }

  /** 给暂缓/先放着的文件补备注（双击行内编辑） */
  async function saveNote(fileId: number, note: string) {
    try {
      await ipc.saveNote(fileId, note)
      const it = items.value.find((x) => x.file.id === fileId)
      if (it) it.file.note = note
      error.value = null
    } catch (e) {
      error.value = toCmdError(e).message
    }
  }

  return {
    view,
    items,
    index,
    current,
    total,
    batchId,
    seq,
    dirCount,
    remaining,
    deferredBadge,
    confirmedCount,
    confirmedList,
    allDone,
    loading,
    error,
    finished,
    inputOpen,
    lastAck,
    undoStack,
    load,
    accept,
    pick,
    newDirConfirm,
    defer,
    park,
    undo,
    move,
    focus,
    openInput,
    closeInput,
    finish,
    saveNote,
  }
})
