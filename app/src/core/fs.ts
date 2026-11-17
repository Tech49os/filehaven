import type { FileItem, ScanProgress } from './types'
import { extractText, extOf, kindOf } from './extractor'

/** 浏览器是否支持目录选择（Chrome/Edge 支持） */
export const supportsFSA =
  typeof window !== 'undefined' && typeof window.showDirectoryPicker === 'function'

/** 系统目录黑名单：强制排除，不允许用户放开（技术设计 §10.1） */
export const SYSTEM_DIRS = [
  'windows',
  'program files',
  'program files (x86)',
  'programdata',
  'appdata',
  'application data',
  '$recycle.bin',
  'system volume information',
  'recovery',
  'perflogs',
  'node_modules',
  '.git',
  '.cache',
  '.workbuddy',
  'boot',
  'drivers',
  'msocache',
]

export function isSystemDir(name: string): boolean {
  return SYSTEM_DIRS.includes(name.toLowerCase())
}

/** 弹出目录选择器 */
export async function pickDirectory(
  mode: 'read' | 'readwrite' = 'readwrite'
): Promise<FileSystemDirectoryHandle | null> {
  if (!supportsFSA) return null
  try {
    return await window.showDirectoryPicker!({ mode })
  } catch {
    return null
  }
}

interface ScanOptions {
  maxSizeBytes: number
  archiveRoot: string
  onProgress?: (p: ScanProgress) => void
  /** 最多扫描文件数（防止出错时无限扫描） */
  limit?: number
  signal?: { cancelled: boolean }
}

/**
 * 递归扫描目录，抽取元数据与正文特征
 * 生产版由 Rust scanner 承担（技术设计 §4.1），此处为 MVP 的前端等价实现
 */
export async function scanDirectory(
  root: FileSystemDirectoryHandle,
  opts: ScanOptions
): Promise<FileItem[]> {
  const out: FileItem[] = []
  const { maxSizeBytes, archiveRoot, onProgress, limit = 5000, signal } = opts
  let processed = 0
  let skipped = 0

  async function visit(
    dir: FileSystemDirectoryHandle,
    basePath: string,
    depth: number
  ): Promise<void> {
    if (depth > 12) return
    const entries: FileSystemHandle[] = []
    // 兼容：部分 TS lib 未声明 async iterator
    for await (const entry of (dir as unknown as {
      values: () => AsyncIterable<FileSystemHandle>
    }).values()) {
      entries.push(entry)
    }

    for (const entry of entries) {
      if (signal?.cancelled) return
      if (out.length >= limit) return
      const fullPath = `${basePath}\\${entry.name}`

      if (entry.kind === 'directory') {
        if (isSystemDir(entry.name)) {
          skipped++
          continue
        }
        if (archiveRoot && fullPath.toLowerCase().startsWith(archiveRoot.toLowerCase())) {
          skipped++
          continue
        }
        await visit(entry as FileSystemDirectoryHandle, fullPath, depth + 1)
        continue
      }

      const fh = entry as FileSystemFileHandle
      let file: File
      try {
        file = await fh.getFile()
      } catch {
        skipped++
        continue
      }
      if (file.size > maxSizeBytes) {
        skipped++
        continue
      }

      processed++
      onProgress?.({
        processed,
        total: processed,
        currentPath: fullPath,
        skipped,
      })

      const kind = kindOf(file.name)
      const feat = await extractText(file, kind)
      out.push({
        id: `h_${hashPath(fullPath)}`,
        name: file.name,
        ext: extOf(file.name),
        kind,
        size: file.size,
        mtime: file.lastModified,
        path: fullPath,
        textSummary: feat.text.slice(0, 2000),
        drawioNodes: feat.drawioNodes,
        parseError: feat.parseError,
        categoryId: null,
        suggestedName: '',
        tags: [],
        confidence: 0,
        needReview: true,
        status: 'scanned',
        handle: fh,
      })
    }
  }

  await visit(root, root.name, 0)
  return out
}

function hashPath(p: string): string {
  let h = 5381
  for (let i = 0; i < p.length; i++) {
    h = ((h << 5) + h + p.charCodeAt(i)) | 0
  }
  return Math.abs(h).toString(36)
}

/** 逐级创建目录 */
export async function ensureDir(
  root: FileSystemDirectoryHandle,
  segments: string[]
): Promise<FileSystemDirectoryHandle> {
  let cur = root
  for (const seg of segments) {
    const safe = sanitizeSegment(seg)
    if (!safe) continue
    cur = await cur.getDirectoryHandle(safe, { create: true })
  }
  return cur
}

export function sanitizeSegment(s: string): string {
  return s.replace(/[\\/:*?"<>|]/g, '_').trim()
}

/** 目标目录下解析同名冲突：存在则加 _1 _2 */
export async function resolveConflict(
  dir: FileSystemDirectoryHandle,
  name: string
): Promise<{ name: string; conflict: boolean }> {
  const dot = name.lastIndexOf('.')
  const base = dot > 0 ? name.slice(0, dot) : name
  const ext = dot > 0 ? name.slice(dot) : ''
  let candidate = name
  let i = 0
  let conflict = false
  // 最多尝试 200 次
  while (i < 200) {
    try {
      await dir.getFileHandle(candidate)
      // 能拿到 → 已存在
      conflict = true
      i++
      candidate = `${base}_${i}${ext}`
    } catch {
      return { name: candidate, conflict }
    }
  }
  return { name: `${base}_${Date.now()}${ext}`, conflict: true }
}

/** 写文件到目录 */
export async function writeFileTo(
  dir: FileSystemDirectoryHandle,
  name: string,
  data: Blob
): Promise<void> {
  const fh = await dir.getFileHandle(name, { create: true })
  const w = await fh.createWritable()
  await w.write(data)
  await w.close()
}

/** 计算 Blob 的摘要（用于完整性校验） */
export async function digestOf(blob: Blob): Promise<string> {
  const buf = await blob.arrayBuffer()
  const hash = await crypto.subtle.digest('SHA-256', buf)
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}

/** 删除文件（移入 .trash 由调用方负责，这里只做底层删除） */
export async function removeFile(
  dir: FileSystemDirectoryHandle,
  name: string
): Promise<void> {
  await dir.removeEntry(name)
}

/** 从路径字符串逐级定位父目录 */
export async function dirAtPath(
  root: FileSystemDirectoryHandle,
  segments: string[]
): Promise<FileSystemDirectoryHandle | null> {
  let cur = root
  try {
    for (const seg of segments) {
      cur = await cur.getDirectoryHandle(seg)
    }
    return cur
  } catch {
    return null
  }
}
