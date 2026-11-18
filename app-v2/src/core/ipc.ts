/**
 * 全部 IPC 命令的 TS 封装 —— 前端只走这一层，页面里禁止直接 `invoke`。
 *
 * 入参一律 camelCase（Rust 侧 `serde(rename_all = "camelCase")`），
 * 结构体入参按 Tauri v2 的约定包在参数名里（如 `{ req }`）。
 */

import { invoke } from '@tauri-apps/api/core'
import type {
  ArchiveDir,
  ArchiveInfo,
  BatchItem,
  BatchView,
  BootState,
  CloseAck,
  DecideAck,
  DecideReq,
  DirNamePreview,
  DirNode,
  DirSuggestion,
  FileRow,
  MigrationPreview,
  MigrationResult,
  ParkedItem,
  ScanReport,
  SourceRoot,
  UndoReport,
} from './types'

// ---------------------------------------------------------------- 基础

export function pickDirectory(title?: string, initial?: string) {
  return invoke<string | null>('pick_directory', { title: title ?? null, initial: initial ?? null })
}

export function openPath(path: string) {
  return invoke<void>('open_path', { path })
}

export function revealPath(path: string) {
  return invoke<void>('reveal_path', { path })
}

export function diskFreeGb(path: string) {
  return invoke<number | null>('disk_free_gb', { path })
}

// ---------------------------------------------------------------- 启动 / 配置

export function bootState() {
  return invoke<BootState>('boot_state')
}

export function addSourceRoot(path: string) {
  return invoke<SourceRoot>('add_source_root', { path })
}

export function removeSourceRoot(id: number) {
  return invoke<void>('remove_source_root', { id })
}

export function listSourceRoots() {
  return invoke<SourceRoot[]>('list_source_roots')
}

export function setArchiveRoot(path: string) {
  return invoke<ArchiveInfo>('set_archive_root', { path })
}

// ---------------------------------------------------------------- 扫描

export function scanAll() {
  return invoke<ScanReport>('scan_all')
}

// ---------------------------------------------------------------- 目录

export function listDirs() {
  return invoke<DirNode[]>('list_dirs')
}

export function previewDirName(input: string) {
  return invoke<DirNamePreview>('preview_dir_name', { input })
}

export function createDir(input: string) {
  return invoke<ArchiveDir>('create_dir', { input })
}

export function acceptDirs(names: string[]) {
  return invoke<ArchiveDir[]>('accept_dirs', { names })
}

/** M5 未接入时后端没有这个命令，失败由页面降级处理 */
export function suggestDirs(force = false) {
  return invoke<DirSuggestion>('suggest_dirs', { force })
}

// ---------------------------------------------------------------- 批次 / 判定

export function nextBatch(size = 12) {
  return invoke<BatchView>('next_batch', { size })
}

export function decide(req: DecideReq) {
  return invoke<DecideAck>('decide', { req })
}

export function undoDecide(batchId: number) {
  return invoke<BatchItem | null>('undo_decide', { batchId })
}

export function closeBatch(batchId: number) {
  return invoke<CloseAck>('close_batch', { batchId })
}

// ---------------------------------------------------------------- 待定区

export function listParked() {
  return invoke<ParkedItem[]>('list_parked')
}

export function assignParked(fileId: number, dirId: number, origin = 'user_picked') {
  return invoke<void>('assign_parked', { fileId, dirId, origin })
}

export function unpark(fileId: number) {
  return invoke<void>('unpark', { fileId })
}

export function dropFile(fileId: number) {
  return invoke<void>('drop_file', { fileId })
}

export function saveNote(fileId: number, note: string) {
  return invoke<void>('save_note', { fileId, note })
}

// ---------------------------------------------------------------- 迁移

export function migrationPreview() {
  return invoke<MigrationPreview>('migration_preview')
}

export function applyMigration(excludeFileIds: number[]) {
  return invoke<MigrationResult>('apply_migration', { excludeFileIds })
}

export function undoMigration(migrationId: number) {
  return invoke<UndoReport>('undo_migration', { migrationId })
}

// ---------------------------------------------------------------- 浏览

export function browseDir(dirId: number | null) {
  return invoke<FileRow[]>('browse_dir', { dirId })
}

export function searchFiles(q: string, dirId: number | null = null, limit = 200) {
  return invoke<FileRow[]>('search_files', { q, dirId, limit })
}
