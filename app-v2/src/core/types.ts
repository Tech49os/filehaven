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
  pendingConfirm: number
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
  /** 运行时监听是否开着（设置页可关） */
  watchEnabled: boolean
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

/** 一个标签：kind 决定样式与可删性（type/time 基础、topic 模型给、user 人写） */
export interface TagRef {
  id: number
  name: string
  kind: 'type' | 'time' | 'topic' | 'keyword' | 'user'
}

/** 标签词表里的一行（设置页 T1.5） */
export interface TagCount {
  id: number
  name: string
  kind: 'user' | 'topic'
  /** 挂了多少份文件 */
  fileCount: number
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
  /** 归档时改成的名字；null = 保持原名 */
  targetName?: string | null
  /** 深度解读的一句话说明；null = 没读过，或读过但没有可说的（看 tagLevel） */
  summary?: string | null
  /** basic（只打了类型/时间标签）| deep（读过正文） */
  tagLevel?: string
  /** 人改过标签：队列只写 summary 不覆盖标签（P3 T1.4） */
  tagLocked?: boolean
  /** 迁移后落盘的归档路径；null = 未归档（打开/定位/预览请用 livePath） */
  archivedPath?: string | null
  /** 迁移落到的归档目录 id；null = 未归档或目录被删 */
  archiveDirId?: number | null
  /** 标签，后端按 type→time→topic→keyword→user 排好；空 = 没有标签，不占位 */
  tags?: TagRef[]
}

/** 已归档用归档路径，其余用源路径 —— 与后端 `FileRow::live_path()` 同一套口径 */
export function livePath(f: Pick<FileRow, 'archivedPath' | 'absPath'>): string {
  return f.archivedPath ?? f.absPath
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

/**
 * 一份文件的预览（`preview_file`）
 *
 * `kind === 'image'` 时 `text` 为空 —— 前端直接拿 `absPath` 走 asset 协议显示缩略图，
 * 不在后端解码。读不出正文时 `reason` 一定有话，前端原样显示。
 */
export interface FilePreview {
  text: string
  truncated: boolean
  reason: string
  kind: 'text' | 'image' | 'none'
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
  /** 没有足够像的目录：得人自己建一个 */
  needsYou: boolean
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
  /** 标注后源文件被改过（或已经不见了），必须拦下 */
  changedSinceLabel: { fileId: number; name: string; why: 'mtime' | 'size' | 'missing' }[]
  totalFiles: number
  totalBytes: number
  /** 归档盘剩余空间（GB）；算不出来就是 null，不拦 */
  freeGb: number | null
  /** 服务端算的「装得下吗」，前端不要自己再猜一遍 */
  enoughSpace: boolean
}

/** 最近一次迁移（撤销入口） */
export interface MigrationSummary {
  id: number
  status: 'running' | 'done' | 'partial' | 'failed' | 'undone'
  fileCount: number
  bytes: number
  finishedAt: number | null
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

/** 深度标签队列的实时状态（`deep-tag-progress` 事件与 `deep_tag_status` 命令同一个形状） */
export interface DeepTagStats {
  done: number
  failed: number
  /** 还剩多少份没深读 */
  remaining: number
  current: string
  /** 正在读哪一份（T4.5）：待定区「正在读…」按钮态靠它收敛 */
  currentId: number | null
  running: boolean
  paused: boolean
  enabled: boolean
  /** 停下来的原因（如「队列已停：模型未响应」） */
  note: string | null
}

// ---------------------------------------------------------------- 事件

export interface ScanProgress {
  rootPath: string
  processed: number
  total: number
  currentPath: string
  skipped: number
}

/** 增量扫描/监听发现了新文件（`files-found`）—— 中性提示，不是错误 */
export interface FilesFound {
  added: number
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
