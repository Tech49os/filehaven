/**
 * Tauri 桌面后端桥接
 *
 * 存在 Tauri 运行时（桌面版）时走 Rust 命令；否则返回 false，
 * 由浏览器 File System Access 实现兜底（仅用于开发调试）。
 */
import type { FileItem, FileKind, MigrationRecord, ScanProgress } from './types'

/* eslint-disable @typescript-eslint/no-explicit-any */
type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<any>
type ListenFn = (
  event: string,
  handler: (e: { payload: any }) => void
) => Promise<() => void>

let invokeFn: InvokeFn | null = null
let listenFn: ListenFn | null = null

/** 是否运行在 Tauri 桌面容器内 */
export const isDesktop =
  typeof window !== 'undefined' &&
  (('__TAURI_INTERNALS__' in window) || ('__TAURI__' in window))

/** 动态加载 @tauri-apps/api，避免浏览器环境报错 */
async function ensureApi(): Promise<boolean> {
  if (!isDesktop) return false
  if (invokeFn && listenFn) return true
  try {
    const core = await import('@tauri-apps/api/core')
    const ev = await import('@tauri-apps/api/event')
    invokeFn = core.invoke as unknown as InvokeFn
    listenFn = ev.listen as unknown as ListenFn
    return true
  } catch {
    return false
  }
}

export async function pickDirectory(
  title = '选择目录',
  initial?: string
): Promise<string | null> {
  if (!(await ensureApi())) return null
  return (await invokeFn!('pick_directory', { title, initial })) ?? null
}

export interface RawScanned {
  name: string
  path: string
  ext: string
  kind: string
  size: number
  mtime: number
  textSummary: string
  drawioNodes: string[]
  parseError: boolean
}

export interface ScanSummary {
  files: RawScanned[]
  skipped: number
  truncated: boolean
}

/** 扫描目录（Rust 侧递归 + 正文抽取），带进度事件 */
export async function scanDirectory(
  root: string,
  opts: {
    archiveRoot?: string
    maxSizeMB?: number
    limit?: number
    onProgress?: (p: ScanProgress) => void
    signal?: { cancelled: boolean }
  } = {}
): Promise<ScanSummary> {
  if (!(await ensureApi())) throw new Error('当前环境不支持本地扫描')

  let unlisten: (() => void) | null = null
  if (opts.onProgress && listenFn) {
    unlisten = await listenFn('scan-progress', (e) => {
      opts.onProgress?.(e.payload as ScanProgress)
    })
  }

  try {
    const res = (await invokeFn!('scan_directory', {
      root,
      archiveRoot: opts.archiveRoot ?? null,
      maxSizeMB: opts.maxSizeMB ?? 2048,
      limit: opts.limit ?? 20000,
    })) as ScanSummary
    return res
  } finally {
    unlisten?.()
  }
}

/** 把 Rust 扫描结果转成前端 FileItem */
export function toFileItems(files: RawScanned[]): FileItem[] {
  return files.map((f) => ({
    id: `f_${hashPath(f.path)}`,
    name: f.name,
    ext: f.ext,
    kind: (f.kind as FileKind) ?? 'other',
    size: f.size,
    mtime: f.mtime,
    path: f.path,
    textSummary: f.textSummary ?? '',
    drawioNodes: f.drawioNodes?.length ? f.drawioNodes : undefined,
    parseError: f.parseError || undefined,
    categoryId: null,
    suggestedName: '',
    tags: [],
    confidence: 0,
    needReview: true,
    status: 'scanned',
  }))
}

function hashPath(p: string): string {
  let h = 5381
  for (let i = 0; i < p.length; i++) h = ((h << 5) + h + p.charCodeAt(i)) | 0
  return Math.abs(h).toString(36)
}

export interface DesktopPlan {
  fileId: string
  fileName: string
  sourcePath: string
  targetDir: string
  targetName: string
}

export interface DesktopMigrationReport {
  batchId: string
  records: MigrationRecord[]
  migrated: number
  failed: number
  bytes: number
  durationMs: number
}

/** 执行三段式迁移 */
export async function migrateFiles(
  archiveRoot: string,
  batchId: string,
  plans: DesktopPlan[],
  onProgress?: (done: number, total: number, name: string, stage: string) => void
): Promise<DesktopMigrationReport> {
  if (!(await ensureApi())) throw new Error('当前环境不支持本地迁移')

  let unlisten: (() => void) | null = null
  if (onProgress && listenFn) {
    unlisten = await listenFn('migrate-progress', (e) => {
      const p = e.payload as { done: number; total: number; name: string; stage: string }
      onProgress(p.done, p.total, p.name, p.stage)
    })
  }

  try {
    const res = (await invokeFn!('migrate_files', {
      archiveRoot,
      batchId,
      plans,
    })) as {
      batchId: string
      records: Array<{
        fileId: string
        fileName: string
        sourcePath: string
        targetPath: string
        status: string
        size: number
        error: string | null
      }>
      migrated: number
      failed: number
      bytes: number
      durationMs: number
    }

    const records: MigrationRecord[] = res.records.map((r, i) => ({
      id: `m_${batchId}_${i}`,
      batchId,
      fileId: r.fileId,
      fileName: r.fileName,
      sourcePath: r.sourcePath,
      targetPath: r.targetPath,
      status: r.status as MigrationRecord['status'],
      size: r.size,
      time: Date.now(),
      error: r.error ?? undefined,
    }))

    return { ...res, records }
  } finally {
    unlisten?.()
  }
}

/** 撤销一个批次 */
export async function undoMigration(
  archiveRoot: string,
  batchId: string,
  records: MigrationRecord[]
): Promise<{ restored: number; failed: number }> {
  if (!(await ensureApi())) throw new Error('当前环境不支持撤销')
  return (await invokeFn!('undo_migration', {
    archiveRoot,
    batchId,
    records: records.map((r) => ({
      fileId: r.fileId,
      fileName: r.fileName,
      sourcePath: r.sourcePath,
      targetPath: r.targetPath,
      status: r.status,
      size: r.size,
      error: r.error ?? null,
    })),
  })) as { restored: number; failed: number }
}

export async function prepareArchiveRoot(path: string): Promise<string> {
  if (!(await ensureApi())) throw new Error('当前环境不支持')
  return (await invokeFn!('prepare_archive_root', { path })) as string
}

export async function openPath(path: string): Promise<void> {
  if (!(await ensureApi())) return
  await invokeFn!('open_path', { path })
}

export async function diskFreeGb(path: string): Promise<number | null> {
  if (!(await ensureApi())) return null
  return (await invokeFn!('disk_free_gb', { path })) as number | null
}

/**
 * 走 Rust 发 HTTP 请求（用于调本地大模型）
 *
 * 桌面端必须走这条路：Windows 上页面 origin 是 `http://tauri.localhost`，
 * Ollama 的 CORS 白名单默认不含它，浏览器直连会 403。
 * 浏览器开发环境没有 Rust 后端，退回普通 fetch。
 */
export async function llmHttp(
  method: 'GET' | 'POST',
  url: string,
  body?: unknown,
  timeoutMs = 60000
): Promise<unknown> {
  if (await ensureApi()) {
    const text = (await invokeFn!('llm_request', {
      method,
      url,
      body: body === undefined ? null : JSON.stringify(body),
      timeoutMs,
    })) as string
    return text ? JSON.parse(text) : null
  }
  const res = await fetch(url, {
    method,
    headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    // 浏览器环境（开发预览）没有 Rust 那层超时，必须自己兜住：
    // 曾经因为这里没有超时，模型被挤出显存后请求挂了 59 分钟都没返回
    signal: AbortSignal.timeout(timeoutMs),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return await res.json()
}

/** 当前是否走 Rust 代理（桌面端） */
export function hasRustBridge(): boolean {
  return isDesktop
}
