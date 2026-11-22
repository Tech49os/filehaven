/**
 * 后端事件订阅（详设 §6.4）。
 *
 * 纯浏览器里跑（vite dev 无 Tauri）时 `listen` 会抛错，
 * 这里兜成「返回一个空 unlisten」，让页面代码不用到处判环境。
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  DeepTagStats,
  DirHealthStage,
  FilesFound,
  MigrateProgress,
  ScanProgress,
  SuggestStage,
} from './types'

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

export function onDeepTagProgress(cb: (p: DeepTagStats) => void) {
  return safeListen<DeepTagStats>('deep-tag-progress', cb)
}

/** 增量扫描或运行时监听发现新文件（T3.7） */
export function onFilesFound(cb: (p: FilesFound) => void) {
  return safeListen<FilesFound>('files-found', cb)
}

export function onMigrateProgress(cb: (p: MigrateProgress) => void) {
  return safeListen<MigrateProgress>('migrate-progress', cb)
}

export function onUndoProgress(cb: (p: MigrateProgress) => void) {
  return safeListen<MigrateProgress>('undo-progress', cb)
}

/** BUG-003 目录体检阶段：scan → rule → model → done */
export function onDirHealthStage(cb: (p: DirHealthStage) => void) {
  return safeListen<DirHealthStage>('dir-health-stage', cb)
}

/** BUG-006 同义目录合并阶段：扫描同上；采纳时按文件发 copy → verify → quarantine */
export function onDirMergeStage(cb: (p: DirHealthStage) => void) {
  return safeListen<DirHealthStage>('dir-merge-stage', cb)
}
