/** 显示格式化的小工具（纯函数，单测覆盖边界） */

/** 1024 进制，保留 1 位小数；0.4KB 这种显示成 410B 更直观 */
export function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  if (bytes < 1024) return `${Math.round(bytes)}B`
  const kb = bytes / 1024
  if (kb < 1024) return `${kb < 10 ? kb.toFixed(1) : Math.round(kb)}KB`
  const mb = kb / 1024
  if (mb < 1024) return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)}MB`
  const gb = mb / 1024
  return `${gb.toFixed(1)}GB`
}

/** 毫秒时间戳 → 2026-11-18 21:30；同年只写月日 */
export function formatTime(ms: number, now = Date.now()): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—'
  const d = new Date(ms)
  const n = new Date(now)
  const pad = (v: number) => String(v).padStart(2, '0')
  const md = `${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
  if (d.getFullYear() !== n.getFullYear()) return `${d.getFullYear()}-${md}`
  return `${md} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** 相对时间：今天 / 昨天 / 3 天前 / 具体日期 */
export function relativeTime(ms: number, now = Date.now()): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—'
  const day = 86400_000
  const diff = now - ms
  if (diff < 0) return formatTime(ms, now)
  if (diff < day && new Date(ms).getDate() === new Date(now).getDate()) return '今天'
  if (diff < 2 * day) return '昨天'
  if (diff < 7 * day) return `${Math.floor(diff / day)} 天前`
  return formatTime(ms, now)
}

/** 扩展名 → 图标名（AppIcon 的键） */
export function iconOf(kind: string): string {
  switch (kind) {
    case 'word':
    case 'excel':
    case 'ppt':
      return 'list'
    case 'pdf':
      return 'info'
    case 'image':
      return 'image'
    case 'diagram':
      return 'layers'
    default:
      return 'info'
  }
}

/** kind → 中文标签（「文档 / 表格 / 演示」比 word/excel 好认） */
export function kindLabel(kind: string): string {
  switch (kind) {
    case 'word':
      return '文档'
    case 'excel':
      return '表格'
    case 'ppt':
      return '演示'
    case 'pdf':
      return 'PDF'
    case 'image':
      return '图片'
    case 'diagram':
      return '图示'
    default:
      return '其他'
  }
}

export function statusLabel(status: string): string {
  switch (status) {
    case 'confirmed':
      return '已确认'
    case 'pending_confirm':
      return '待确认'
    case 'labeled':
      return '待整理'
    case 'queued':
      return '已暂缓'
    case 'parked':
      return '先放着'
    case 'migrated':
      return '已迁移'
    case 'skipped':
      return '已跳过'
    case 'missing':
      return '找不到了'
    default:
      return status
  }
}
