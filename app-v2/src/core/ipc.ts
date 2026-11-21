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
  DeepTagStats,
  DirSuggestion,
  FilePreview,
  FileRow,
  MigrationPreview,
  MigrationResult,
  MigrationSummary,
  ParkedItem,
  ScanReport,
  SourceRoot,
  TagCount,
  TagRef,
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

/**
 * 启动状态。
 *
 * `probeModel` 只在**启动首调**传 true —— 探活是同步 HTTP（最长 3s），
 * 而本函数在每次判定后都会被调来刷计数，挂在那就等于每按一次回车卡一下。
 * false 时后端直接返回上次的探活结果。
 */
export function bootState(probeModel = false) {
  return invoke<BootState>('boot_state', { probeModel })
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
export function suggestDirs(force = false, hint = '') {
  return invoke<DirSuggestion>('suggest_dirs', { force, hint: hint.trim() || null })
}

/**
 * 一份文件的正文预览
 *
 * 700 字上限、图片只回标记（缩略图走 asset 协议）、读不出正文时给原因。
 * 前端按 fileId 缓存结果，同一份文件一次会话只读一次。
 */
export function previewFile(fileId: number) {
  return invoke<FilePreview>('preview_file', { fileId })
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

/**
 * 归档前改名（只改库里的目标名，源文件不动）。
 *
 * 传空串 = 改回原名。返回后端清洗后的名字（扩展名会补回来）。
 */
export function renameFile(fileId: number, newName: string) {
  return invoke<string>('rename_file', { fileId, newName })
}

/**
 * 按**现在的**目录集合重算这几份文件的候选。
 *
 * 目录新增/用过之后，本来算好的候选就过期了 —— 不重开批次，只把候选换掉，
 * 已确认的、焦点位置、撤销栈都不动。
 */
export function rescoreFiles(fileIds: number[]) {
  return invoke<BatchItem[]>('rescore_files', { fileIds })
}

// ---------------------------------------------------------------- 深度解读

/** 队列现在的进度（失败数、还剩多少、停下来的原因） */
export function deepTagStatus() {
  return invoke<DeepTagStats>('deep_tag_status')
}

/** 开关 / 暂停（`enabled` 会写库，重启后还是用户的意愿） */
export function setDeepTag(opts: { enabled?: boolean; paused?: boolean }) {
  return invoke<DeepTagStats>('set_deep_tag', {
    enabled: opts.enabled ?? null,
    paused: opts.paused ?? null,
  })
}

// ---------------------------------------------------------------- 监听

/**
 * 开关运行时监听。会写库（重启后仍按用户的意愿），并**当场**起/停监听线程 ——
 * 不是「下次启动才生效」。
 */
export function setWatchEnabled(enabled: boolean) {
  return invoke<void>('set_watch_enabled', { enabled })
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

/** 最近一次迁移：撤销入口要靠它，重启后也还撤得回来 */
export function latestMigration() {
  return invoke<MigrationSummary | null>('latest_migration')
}

/** 标注后被改过的文件：退回整理台，并按磁盘现状刷新 size/mtime */
export function requeueConfirmed(fileId: number) {
  return invoke<void>('requeue_confirmed', { fileId })
}

// ---------------------------------------------------------------- 浏览

export function browseDir(dirId: number | null) {
  return invoke<FileRow[]>('browse_dir', { dirId })
}

export function searchFiles(q: string, dirId: number | null = null, limit = 200) {
  return invoke<FileRow[]>('search_files', { q, dirId, limit })
}

// ---------------------------------------------------------------- 标签（P3 M1）

/** 人加一个标签：一律落 kind='user'，并顺手锁定该文件的标签 */
export function addTag(fileId: number, name: string) {
  return invoke<TagRef>('add_tag', { fileId, name })
}

/** 删一个标签：type/time 会被后端以 TAG_LOCKED 拒掉 */
export function removeTag(fileId: number, tagId: number) {
  return invoke<void>('remove_tag', { fileId, tagId })
}

/** 标签词表（设置页）：user/topic 两类 + 使用份数 */
export function listTags() {
  return invoke<TagCount[]>('list_tags')
}

/** 从词表删除（级联清 file_tags），返回清掉的份数 */
export function deleteTag(tagId: number) {
  return invoke<number>('delete_tag', { tagId })
}

/** 改名；撞名则合并到已有标签，返回合并后的 tag id */
export function renameTag(tagId: number, name: string) {
  return invoke<number>('rename_tag', { tagId, name })
}
