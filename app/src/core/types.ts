/**
 * 核心数据类型
 * 对应技术设计文档 v1.0 第 3 节数据库结构（MVP 用前端存储承载）
 */

export type FileKind =
  | 'doc'
  | 'xls'
  | 'ppt'
  | 'pdf'
  | 'image'
  | 'drawio'
  | 'text'
  | 'code'
  | 'other'

export type FileStatus =
  | 'scanned'
  | 'classified'
  | 'confirmed'
  | 'pending'
  | 'migrated'
  | 'skipped'

/** 一份被扫描到的文件 */
export interface FileItem {
  id: string
  name: string
  ext: string
  kind: FileKind
  size: number
  mtime: number
  /** 原始完整路径（展示用） */
  path: string
  /** 内容摘要（≤2000 字，供分类使用） */
  textSummary: string
  /** draw.io 图内节点文字（差异化能力） */
  drawioNodes?: string[]
  /** draw.io 解析失败 */
  parseError?: boolean
  /** 归入的分类 id */
  categoryId: string | null
  /** AI 命名建议 */
  suggestedName: string
  tags: string[]
  /** 置信度 0–1 */
  confidence: number
  /** 是否需要人工确认（<阈值） */
  needReview: boolean
  status: FileStatus
  /** 真实模式下的文件句柄 */
  handle?: FileSystemFileHandle
  /** 该文件的分类依据（用于 UI 解释） */
  reason?: string
  /** 最终归属的工作域（一级） */
  ws?: string | null
  /** 最终归属的文档类型（二级） */
  dt?: string | null
  /** 结论来源：规则直通 / 模型补判 / 双路一致 / 双路分歧 */
  aiSource?: AiSource
  /** 模型单独给出的判断（用于对照展示） */
  llmWs?: string | null
  llmDt?: string | null
}

/** 单个文件结论的来源 */
export type AiSource = 'rule' | 'llm' | 'agree' | 'conflict' | 'none'

/** 分类节点（两层树） */
export interface Category {
  id: string
  name: string
  desc: string
  parentId: string | null
  order: number
  isFromAI: boolean
}

export interface CategoryNode extends Category {
  children: CategoryNode[]
  /** 直接归属本分类的文件数（含子级累计另算） */
  count: number
  totalCount: number
}

/** 迁移计划的一条 */
export interface MigrationPlan {
  fileId: string
  fileName: string
  sourcePath: string
  targetDir: string
  targetName: string
  /** 目标已存在同名，自动加序号 */
  conflict: boolean
  /** 文件名被规范化过（版本号挪到末尾 / 同名补副本号），界面上要给个交代 */
  renamed?: boolean
  /** 因待确认而暂不迁移 */
  deferred: boolean
}

export type MigrationStatus =
  | 'copied'
  | 'verified'
  | 'recycled'
  | 'reverted'
  | 'failed'

export interface MigrationRecord {
  id: string
  batchId: string
  fileId: string
  fileName: string
  sourcePath: string
  targetPath: string
  status: MigrationStatus
  size: number
  time: number
  error?: string
}

/** 扫描配置 */
export interface ScanConfig {
  rootPath: string
  /** 系统目录黑名单（强制排除） */
  blacklist: string[]
  maxFileSizeMB: number
  /** 排除归档根自身（防递归） */
  archiveRootPath: string
}

export interface ScanProgress {
  processed: number
  total: number
  currentPath: string
  skipped: number
}

/** 归纳结果 */
export interface InductionResult {
  categories: Category[]
  /** 每个文件的归属结果已回写到 files */
  fileCount: number
  autoK: number
  silhouette: number
  byKind: Record<string, number>
}

/** 迁移执行报告 */
export interface MigrationReport {
  batchId: string
  migrated: number
  deferred: number
  failed: number
  bytes: number
  records: MigrationRecord[]
  durationMs: number
}

/** 应用运行模式 */
export type AppMode = 'real' | 'demo'

export interface AppSettings {
  rootPath: string
  rootName: string
  mode: AppMode
  confidenceThreshold: number
  autoOrganizeEnabled: boolean
  idleThresholdSec: number
  requirePower: boolean
  maxFileSizeMB: number
  /** ===== 本地大模型（Ollama）===== */
  /** 是否启用大模型参与分类 */
  llmEnabled: boolean
  llmBaseUrl: string
  llmModel: string
  /** 仅用于 OpenAI 兼容服务，本地 Ollama 留空即可 */
  llmApiKey: string
  /** 并发请求数（显存有限，建议 1–3） */
  llmConcurrency: number
  /** smart=只让模型看规则没把握的文件；all=全部双路交叉验证 */
  llmStrategy: 'smart' | 'all'
  /** 单次最多送模型的文件数，0 = 不限 */
  llmMaxFiles: number
}

/** 混合归纳的统计 */
export interface HybridStats {
  total: number
  /** 模型是否真的参与了 */
  llmUsed: boolean
  /** 送模型的文件数 */
  llmCandidates: number
  /** 模型实际判完的数量 */
  llmDone: number
  /** 双路一致 */
  agree: number
  /** 双路分歧（进待确认） */
  conflict: number
  /** 模型补盲：规则判不出、模型判出来了 */
  filled: number
  /** 纯规则判定 */
  ruleOnly: number
  /** 始终判不出 */
  unknown: number
  elapsedMs: number
  model: string
  cancelled: boolean
  /** 降级原因（模型不可用等） */
  fallbackReason?: string
}

/** 归纳进度（供 UI 实时展示） */
export interface HybridProgress {
  phase: 'rules' | 'warmup' | 'llm' | 'tree' | 'done'
  done: number
  total: number
  current: string
  /** 刚判完的一条结果 */
  event?: {
    id: string
    name: string
    ws: string | null
    dt: string | null
    confidence: number
    source: AiSource
    reason: string
  }
}
