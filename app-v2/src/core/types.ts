/**
 * IPC 契约的 TS 侧定义 —— 与 `src-tauri/src/model.rs` 一一对应（详设 §6.3）。
 *
 * 命名规则：Rust 侧全部 `serde(rename_all = "camelCase")`，
 * 所以这里只用 camelCase；ipc.ts 传参也用同一套名字，禁止出现 file_id。
 */

export type FileStatus =
  | 'queued'
  | 'labeled'
  | 'pending_confirm'
  | 'confirmed'
  | 'migrated'
  | 'parked'
  | 'skipped'
  | 'missing'

/** 目录来源：导入的、模型方案采纳的、用户自建的、卡片里顺手建的 */
export type DirOrigin = 'imported' | 'model_approved' | 'user' | 'inline'

/** 判定来源 */
export type DecideOrigin = 'recommended' | 'user_changed' | 'user_picked' | 'new_dir'

export type Decision = 'accepted' | 'changed' | 'deferred' | 'parked'

export type FileKind = 'word' | 'excel' | 'ppt' | 'pdf' | 'image' | 'diagram' | 'other'

export interface SourceRoot {
  id: number
  path: string
  enabled: boolean
}

export interface Counts {
  queued: number
  labeled: number
  pending_confirm: number
  confirmed: number
  migrated: number
  parked: number
  skipped: number
  missing: number
}

export interface ModelStatus {
  reachable: boolean
  model: string | null
}

export interface BootState {
  configured: boolean
  archiveRoot: string | null
  sourceRoots: SourceRoot[]
  counts: Counts
  model: ModelStatus
}

export interface ArchiveInfo {
  path: string
  freeGb: number | null
  quarantinePath: string
}

export interface ScannedFile {
  name: string
  path: string
  ext: string
  kind: FileKind
  size: number
  mtime: number
}

export interface ScanReport {
  scanned: number
  inserted: number
  updated: number
  missing: number
  skippedByRule: number
  elapsedMs: number
}

export interface FileRow {
  id: number
  name: string
  ext: string
  kind: FileKind
  size: number
  mtime: number
  status: FileStatus
  relPath: string
  absPath: string
  sourceRootId: number | null
  deferCount: number
  note: string | null
}

export interface ArchiveDir {
  id: number
  parentId: number | null
  name: string
  relPath: string
  depth: number
  origin: DirOrigin
  createdAt: number
  lastUsedAt: number | null
}

export interface DirNode {
  id: number
  name: string
  relPath: string
  depth: number
  origin: DirOrigin
  fileCount: number
  children: DirNode[]
}

export interface DirNamePreview {
  cleaned: string
  valid: boolean
  reason: string | null
}

export interface DirSuggestion {
  /** 方案组：每组一个方案名 */
  optionA: SuggestedDir[]
  optionB: SuggestedDir[]
  /** 两套方案的维度名（如「按事项」/「按文档用途」），列标题显示 */
  dimensionA: string
  dimensionB: string
  model: string
  elapsedMs: number
  /** 实际送模型的样本数 */
  sampleCount?: number
}

export interface SuggestedDir {
  name: string
  /** 依据文案（模型给的或规则拼的），必须非空 */
  basis: string
  fileCount: number
}

export interface Candidate {
  dirId: number
  relPath: string
  score: number
  /** 非空，服务端保证 */
  reason: string
  /** 0.35 ≤ score < 0.55 → 橙色徽标 */
  uncertain: boolean
}

export interface BatchItem {
  file: FileRow
  /** 0~2 个备选，主推在 [0]；needsYou 时为空 */
  candidates: Candidate[]
  needsYou: boolean
}

export interface BatchView {
  batchId: number
  seq: number
  size: number
  items: BatchItem[]
  remainingInPool: number
  /** 0 时切空态（验收 11） */
  dirCount: number
  /** deferCount ≥ 3 的 fileId，排最后并标注 */
  deferredBadge: number[]
}

export interface DecideReq {
  fileId: number
  batchId: number
  dirId: number | null
  decision: Decision
  origin: DecideOrigin
  confidence: number | null
  reason: string | null
  note?: string
}

export interface DecideAck {
  fileId: number
  status: FileStatus
  /** 本批已确认份数 */
  confirmed: number
  targetRelPath: string | null
  remainingInPool: number
}

export interface CloseAck {
  closed: number
  remainingInPool: number
}

export interface ParkedItem {
  file: FileRow
  candidates: Candidate[]
}

export interface MigrationPreview {
  groups: {
    dirId: number
    relPath: string
    isNew: boolean
    files: FileRow[]
    bytes: number
  }[]
  /** 将加 _2 的冲突 */
  nameConflicts: { fileId: number; targetName: string }[]
  /** 标注后源文件被改过，必须拦下 */
  changedSinceLabel: { fileId: number; name: string; why: 'mtime' | 'size' }[]
  totalFiles: number
  totalBytes: number
}

export interface MigrationResult {
  migrationId: number
  copied: number
  verified: number
  quarantined: number
  failed: { fileId: number; name: string; reason: string }[]
}

export interface UndoReport {
  restored: number
  skipped: number
  failed: { fileId: number; name: string; reason: string }[]
}

// ---------------------------------------------------------------- 事件

export interface ScanProgress {
  rootPath: string
  processed: number
  total: number
  currentPath: string
  skipped: number
}

export interface SuggestStage {
  stage: 'sampling' | 'waiting' | 'validating'
  elapsedMs: number
  note: string
}

export interface MigrateProgress {
  done: number
  total: number
  name: string
  stage: 'copy' | 'verify' | 'quarantine'
}
