/**
 * 后端事件订阅（详设 §6.4）。
 *
 * 纯浏览器里跑（vite dev 无 Tauri）时 `listen` 会抛错，
 * 这里兜成「返回一个空 unlisten」，让页面代码不用到处判环境。
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { MigrateProgress, ScanProgress, SuggestStage } from './types'

async function safeListen<T>(name: string, cb: (p: T) => void): Promise<UnlistenFn> {
  try {
    return await listen<T>(name, (e) => cb(e.payload))
  } catch {
    return () => {}
  }
}

export function onScanProgress(cb: (p: ScanProgress) => void) {
  return safeListen<ScanProgress>('scan-progress', cb)
}

export function onSuggestStage(cb: (p: SuggestStage) => void) {
  return safeListen<SuggestStage>('suggest-stage', cb)
}

export function onMigrateProgress(cb: (p: MigrateProgress) => void) {
  return safeListen<MigrateProgress>('migrate-progress', cb)
}

export function onUndoProgress(cb: (p: MigrateProgress) => void) {
  return safeListen<MigrateProgress>('undo-progress', cb)
}
