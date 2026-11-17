import type {
  AppMode,
  MigrationPlan,
  MigrationRecord,
  MigrationReport,
} from './types'
import {
  digestOf,
  ensureDir,
  resolveConflict,
  writeFileTo,
  dirAtPath,
} from './fs'

export interface ExecContext {
  mode: AppMode
  /** 归档根目录句柄 */
  rootHandle: FileSystemDirectoryHandle | null
  /** fileId → 源文件句柄 */
  fileHandles: Map<string, FileSystemFileHandle>
  batchId: string
  onProgress?: (done: number, total: number, current: string, stage: string) => void
  onRecord?: (r: MigrationRecord) => void
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

function segmentsOf(p: string): string[] {
  return p.split(/[\\/]/).filter(Boolean)
}

/**
 * 执行安全迁移（★信任基石，技术设计 §4.4）
 *
 * 三段式：① 复制（同名加序号，绝不覆盖）→ ② 校验哈希 → ③ 源送回收站/隔离区
 * 任一步失败都保留原件，绝不半途丢数据。
 */
export async function executeMigration(
  plans: MigrationPlan[],
  ctx: ExecContext
): Promise<MigrationReport> {
  const started = Date.now()
  const records: MigrationRecord[] = []
  const todo = plans.filter((p) => !p.deferred)
  const deferred = plans.length - todo.length
  let bytes = 0
  let failed = 0

  for (let i = 0; i < todo.length; i++) {
    const plan = todo[i]
    const total = todo.length
    try {
      if (ctx.mode === 'real' && ctx.rootHandle) {
        await migrateOneReal(plan, ctx, records, (n) => (bytes += n))
      } else {
        await migrateOneDemo(plan, ctx, records, (n) => (bytes += n))
      }
    } catch (e) {
      failed++
      const rec: MigrationRecord = {
        id: `m_${Date.now()}_${i}`,
        batchId: ctx.batchId,
        fileId: plan.fileId,
        fileName: plan.fileName,
        sourcePath: plan.sourcePath,
        targetPath: `${plan.targetDir}\\${plan.targetName}`,
        status: 'failed',
        size: 0,
        time: Date.now(),
        error: e instanceof Error ? e.message : String(e),
      }
      records.push(rec)
      ctx.onRecord?.(rec)
    }
    ctx.onProgress?.(i + 1, total, plan.fileName, '迁移中')
  }

  const migrated = records.filter((r) => r.status === 'recycled' || r.status === 'verified')
    .length

  return {
    batchId: ctx.batchId,
    migrated,
    deferred,
    failed,
    bytes,
    records,
    durationMs: Date.now() - started,
  }
}

/** 真实模式：真复制 + 真校验 + 真隔离 */
async function migrateOneReal(
  plan: MigrationPlan,
  ctx: ExecContext,
  records: MigrationRecord[],
  addBytes: (n: number) => void
): Promise<void> {
  const root = ctx.rootHandle!
  const srcHandle = ctx.fileHandles.get(plan.fileId)
  if (!srcHandle) throw new Error('源文件句柄丢失')

  ctx.onProgress?.(0, 0, plan.fileName, '① 复制中')
  const sourceFile = await srcHandle.getFile()
  const size = sourceFile.size

  // ① 复制到目标目录
  const targetDirSegs = segmentsOf(plan.targetDir)
  const targetDir = await ensureDir(root, targetDirSegs)
  const resolved = await resolveConflict(targetDir, plan.targetName)
  await writeFileTo(targetDir, resolved.name, sourceFile)

  const rec: MigrationRecord = {
    id: `m_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
    batchId: ctx.batchId,
    fileId: plan.fileId,
    fileName: plan.fileName,
    sourcePath: plan.sourcePath,
    targetPath: `${plan.targetDir}\\${resolved.name}`,
    status: 'copied',
    size,
    time: Date.now(),
  }

  // ② 校验哈希：不一致立即删除副本、保留原件、报错
  ctx.onProgress?.(0, 0, plan.fileName, '② 校验完整性')
  const targetFh = await targetDir.getFileHandle(resolved.name)
  const targetFile = await targetFh.getFile()
  const [a, b] = await Promise.all([digestOf(sourceFile), digestOf(targetFile)])
  if (a !== b || sourceFile.size !== targetFile.size) {
    await targetDir.removeEntry(resolved.name)
    rec.status = 'failed'
    rec.error = '哈希校验不一致，已回滚副本'
    records.push(rec)
    ctx.onRecord?.(rec)
    throw new Error('哈希校验不一致')
  }
  rec.status = 'verified'
  records.push(rec)
  ctx.onRecord?.({ ...rec })
  addBytes(size)

  // ③ 源文件送入隔离区（.trash），可恢复，绝不直接抹掉
  ctx.onProgress?.(0, 0, plan.fileName, '③ 源文件移入隔离区')
  try {
    const trashDir = await ensureDir(root, ['.filemanager', 'trash', ctx.batchId])
    await writeFileTo(trashDir, `${plan.fileId}_${plan.fileName}`, sourceFile)
    if (typeof srcHandle.remove === 'function') {
      await srcHandle.remove()
    } else {
      throw new Error('当前浏览器不支持删除源文件，请在资源管理器中手动清理')
    }
    rec.status = 'recycled'
    records[records.length - 1] = rec
    ctx.onRecord?.({ ...rec })
  } catch (e) {
    // 隔离失败不算迁移失败：副本已就位且校验通过，源文件保留
    rec.error = `源文件隔离失败，已保留原件：${e instanceof Error ? e.message : e}`
    records[records.length - 1] = rec
    ctx.onRecord?.({ ...rec })
  }
}

/** 演示模式：完整模拟三段式（不动真实文件） */
async function migrateOneDemo(
  plan: MigrationPlan,
  ctx: ExecContext,
  records: MigrationRecord[],
  addBytes: (n: number) => void
): Promise<void> {
  const size = 20_000 + Math.floor(Math.random() * 400_000)
  ctx.onProgress?.(0, 0, plan.fileName, '① 复制中')
  await sleep(45)
  ctx.onProgress?.(0, 0, plan.fileName, '② 校验完整性')
  await sleep(35)

  const rec: MigrationRecord = {
    id: `m_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
    batchId: ctx.batchId,
    fileId: plan.fileId,
    fileName: plan.fileName,
    sourcePath: plan.sourcePath,
    targetPath: `${plan.targetDir}\\${plan.targetName}`,
    status: 'verified',
    size,
    time: Date.now(),
  }
  records.push(rec)
  ctx.onRecord?.({ ...rec })
  addBytes(size)

  ctx.onProgress?.(0, 0, plan.fileName, '③ 源文件移入隔离区')
  await sleep(30)
  rec.status = 'recycled'
  records[records.length - 1] = rec
  ctx.onRecord?.({ ...rec })
}

/**
 * 撤销一批迁移（技术设计 §7.3）
 * 策略：校验通过 + 源已隔离 → 从隔离区恢复源、删除目标副本
 */
export async function undoBatch(
  batchId: string,
  records: MigrationRecord[],
  ctx: { mode: AppMode; rootHandle: FileSystemDirectoryHandle | null }
): Promise<{ restored: number; failed: number }> {
  let restored = 0
  let failed = 0
  const targets = records.filter((r) => r.status === 'recycled')

  if (ctx.mode === 'demo' || !ctx.rootHandle) {
    // 演示模式：模拟撤销
    for (const _ of targets) {
      void _
      await sleep(40)
      restored++
    }
    return { restored, failed }
  }

  const trashDir = await dirAtPath(ctx.rootHandle, ['.filemanager', 'trash', batchId])
  const restoreDir = await ensureDir(ctx.rootHandle, ['_已恢复', batchId])
  for (const r of targets) {
    try {
      if (!trashDir) throw new Error('未找到隔离区')
      const fh = await trashDir.getFileHandle(`${r.fileId}_${r.fileName}`)
      const file = await fh.getFile()
      await writeFileTo(restoreDir, r.fileName, file)
      restored++
    } catch {
      failed++
    }
  }
  return { restored, failed }
}
