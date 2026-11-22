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
  TrashOutcome,
} from '../core/types'

/** BUG-007：一次删除的本地撤销凭据（文件已从批次移除，撤销时要按原位插回） */
interface TrashUndo {
  fileId: number
  migrationId: number
  item: BatchItem
  index: number
  outcome: TrashOutcome
}

export const useBatchStore = defineStore('batch', () => {
  const view = ref<BatchView | null>(null)
  const index = ref(0)
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** 已处理但有别于 confirmed 的（暂缓 / 先放着），避免焦点又落回它们 */
  const resolved = ref<Set<number>>(new Set())
  const undoStack = ref<number[]>([])
  const finished = ref<CloseAck | null>(null)
  /** 已经「结束本批」：下次进整理台要开新批 */
  const closed = ref(false)
  /** 新建目录输入框是否打开（决定键盘谁说话） */
  const inputOpen = ref(false)
  const lastAck = ref<DecideAck | null>(null)
  /** BUG-007：最近一次删除的撤销凭据（删除后文件从批次消失，靠它插回） */
  const trashUndo = ref<TrashUndo | null>(null)

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
  /** BUG-007：有没有可撤销的删除（Ctrl+Z 与提示条按钮共用） */
  const canUndoTrash = computed(() => trashUndo.value !== null)

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
    closed.value = false
    resolved.value = new Set()
    undoStack.value = []
    trashUndo.value = null
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

  /**
   * 进整理台时用这个，别直接用 `load`。
   *
   * 2026-11-20 反馈：「每次进入整理台他都给我刷新一遍，没有必要」。
   * 批次只该在三种情况下换：还没开过、用户点了「换一批」、上一批已经「结束本批」。
   * 切页面来回走不该丢队列、丢已确认列表、丢撤销栈。
   *
   * 但**空批次不算有批次**：启动时池子里还没有文件，那次 `next_batch` 是一份都没有的，
   * 之后扫完再进来必须重开 —— 否则界面永远停在「池子里没有待整理的文件」。
   */
  async function ensureLoaded(size = 12) {
    if (view.value && !closed.value && view.value.items.length > 0) return view.value
    return load(size)
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
      const ack = await apply(d.id, 'changed', 'new_dir', null)
      // 目录集合变了：剩下那些文件的候选是按旧目录算的，立刻重算一遍
      if (ack) await refreshCandidates()
      return ack
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

  /**
   * BUG-007：删除（移入隔离区）。成功后文件从当前批次消失，
   * 本地留下撤销凭据；失败则批次纹丝不动，不产生幽灵状态。
   */
  async function trash(fileId: number): Promise<TrashOutcome | null> {
    if (!view.value) return null
    const at = items.value.findIndex((x) => x.file.id === fileId)
    if (at < 0) return null
    try {
      const outcome = await ipc.trashFile(fileId)
      const item = items.value[at]
      view.value.items.splice(at, 1)
      trashUndo.value = { fileId, migrationId: outcome.migrationId, item, index: at, outcome }
      if (lastAck.value?.fileId === fileId) lastAck.value = null
      if (index.value >= at && index.value > 0) index.value -= 1
      if (index.value >= items.value.length) index.value = Math.max(0, items.value.length - 1)
      error.value = null
      return outcome
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    }
  }

  /** BUG-007：撤销删除（提示条按钮 / Ctrl+Z 都走这里） */
  async function undoTrash(migrationId?: number): Promise<boolean> {
    const rec = trashUndo.value
    if (!rec) return false
    if (migrationId !== undefined && migrationId !== rec.migrationId) return false
    try {
      await ipc.undoTrash(rec.migrationId)
      const at = Math.min(rec.index, items.value.length)
      view.value?.items.splice(at, 0, rec.item)
      trashUndo.value = null
      index.value = at
      error.value = null
      if (rec.item.file.status === 'confirmed') {
        undoStack.value = pushUndo(undoStack.value, rec.fileId)
      }
      return true
    } catch (e) {
      error.value = toCmdError(e).message
      return false
    }
  }

  /** Ctrl+Z：优先撤销最近的删除，否则撤销最近一次确认并回填 */
  async function undo() {
    if (trashUndo.value) {
      await undoTrash()
      return
    }
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
      closed.value = true
      return ack
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    }
  }

  /**
   * 按现在的目录集合重算候选（新建目录后自动调，也可手动点「重新推荐」）。
   *
   * 只换 candidates / needsYou，**不动**顺序、状态、当前焦点 ——
   * 换掉整批会让已经确认过的东西重新跳一遍，用户会以为点错了。
   * 返回「首选目录变了」的份数，用来给一句反馈。
   */
  async function refreshCandidates() {
    const ids = items.value.filter((i) => !isResolved(i)).map((i) => i.file.id)
    if (ids.length === 0) return 0
    try {
      const fresh = await ipc.rescoreFiles(ids)
      const byId = new Map(fresh.map((b) => [b.file.id, b]))
      let changed = 0
      for (const it of items.value) {
        const f = byId.get(it.file.id)
        if (!f) continue
        if (f.candidates[0]?.dirId !== it.candidates[0]?.dirId) changed += 1
        it.candidates = f.candidates
        it.needsYou = f.needsYou
      }
      error.value = null
      return changed
    } catch (e) {
      error.value = toCmdError(e).message
      return 0
    }
  }

  /** 归档前改名：改的是目标名，源文件不动 */
  async function renameTarget(fileId: number, name: string) {
    const it = items.value.find((x) => x.file.id === fileId)
    if (!it) return null
    try {
      const cleaned = await ipc.renameFile(fileId, name)
      it.file.targetName = cleaned || null
      error.value = null
      return cleaned
    } catch (e) {
      error.value = toCmdError(e).message
      return null
    }
  }

  /**
   * 目录数从 0 变成有时修一下。
   *
   * 批次是复用的，`dirCount` 还停在开批那一刻；如果开批时一个目录都没有，
   * 之后建了目录卡片还会一直卡在「还没有任何目录」。
   * 只在「原来是 0」时改：非 0 的数字差一两个不影响任何界面判断，
   * 而每次挂载都去覆盖它，反而会把好数据冲掉（测试里就是这么炸的）。
   */
  function syncDirCount(n: number) {
    if (!view.value) return
    if (view.value.dirCount > 0) return
    if (n <= 0) return
    view.value = { ...view.value, dirCount: n }
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
    canUndoTrash,
    trashUndo,
    allDone,
    loading,
    error,
    finished,
    closed,
    inputOpen,
    lastAck,
    undoStack,
    load,
    ensureLoaded,
    accept,
    pick,
    newDirConfirm,
    defer,
    park,
    trash,
    undoTrash,
    undo,
    move,
    focus,
    openInput,
    closeInput,
    finish,
    saveNote,
    refreshCandidates,
    renameTarget,
    syncDirCount,
  }
})
