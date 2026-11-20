/**
 * 全局状态：启动态、计数、模型状态（T6.4）
 *
 * P1 不做增量订阅：任何写操作之后调一次 `refreshBoot()`，数字就不会撒谎。
 */

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import * as ipc from '../core/ipc'
import { toCmdError } from '../core/cmdError'
import type { BootState, Counts, ModelStatus } from '../core/types'

const EMPTY_COUNTS: Counts = {
  queued: 0,
  labeled: 0,
  pending_confirm: 0,
  confirmed: 0,
  migrated: 0,
  parked: 0,
  skipped: 0,
  missing: 0,
}

export const useAppStore = defineStore('app', () => {
  const boot = ref<BootState | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const ready = computed(() => boot.value !== null)
  const configured = computed(() => boot.value?.configured ?? false)
  const counts = computed<Counts>(() => boot.value?.counts ?? EMPTY_COUNTS)
  const archiveRoot = computed(() => boot.value?.archiveRoot ?? null)
  const sourceRoots = computed(() => boot.value?.sourceRoots ?? [])
  const modelStatus = computed<ModelStatus>(
    () => boot.value?.model ?? { reachable: false, model: null },
  )

  /** 侧边栏「待迁移 N」：已确认但还没搬走的份数 */
  const pendingMigration = computed(() => counts.value.confirmed)
  /** 侧边栏「待定区 N」 */
  const parkedCount = computed(() => counts.value.parked)
  /** 还没处理的（含排队的） */
  const pendingTotal = computed(
    () => counts.value.labeled + counts.value.pending_confirm + counts.value.queued,
  )

  /**
   * 刷新启动态。`probe` 只在**启动首调**给 true。
   *
   * 默认不探模型：本函数在每次判定后都要调一次（数字不能撒谎），
   * 而探活是同步 HTTP + 3s 超时 —— 放在这条路上就是「每按一次回车停一下」。
   */
  async function refreshBoot(opts: { probe?: boolean } = {}): Promise<BootState | null> {
    loading.value = true
    try {
      boot.value = await ipc.bootState(opts.probe === true)
      error.value = null
      return boot.value
    } catch (e) {
      // 刷新失败不清空旧数据：宁可显示上一次的数字，也不要闪成 0
      error.value = toCmdError(e).message
      return null
    } finally {
      loading.value = false
    }
  }

  function clearError() {
    error.value = null
  }

  return {
    boot,
    loading,
    error,
    ready,
    configured,
    counts,
    archiveRoot,
    sourceRoots,
    modelStatus,
    pendingMigration,
    parkedCount,
    pendingTotal,
    refreshBoot,
    clearError,
  }
})
