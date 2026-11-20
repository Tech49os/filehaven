/**
 * 文本截断的共用规则（P2 T5.2）
 *
 * P1 里三处显示路径的地方各自写了一版：有的靠 CSS 尾省略（`C:\Users\ki…`），
 * 有的干脆不截断。尾省略最要命的是**把最有信息量的末尾几段吃掉** ——
 * `C:\Users\kingb\Desktop\2026\管理\安全检查通知.docx` 截完只剩
 * `C:\Users\kingb\Desk…`，「哪一份」反而看不见了。
 *
 * 规则：保留**盘符/首段** + **末尾 keepTail 段**，中间用 `…` 顶掉。
 * 这样左边知道在哪块盘、右边知道是哪份文件。
 */

const SEP = /[\\/]/

/** 拆路径（同时认 `/` 与 `\`，不改写原分隔符） */
export function splitPath(p: string): string[] {
  return p.split(SEP).filter((s) => s.length > 0)
}

/**
 * 路径中间省略。
 *
 * - 段数不多**且**整串不长 → 原样返回（短路径没必要动它）
 * - 否则 → `首段/…/末两段`（保留原始分隔符风格，Windows 路径仍是反斜杠，
 *   POSIX 绝对路径保留开头的 `/`）
 */
export function shortPath(p: string, keepHead = 1, keepTail = 2): string {
  if (!p) return ''
  const parts = splitPath(p)
  // 40 字符以下不折腾：屏幕放得下，省略了反而看不出是哪份
  if (parts.length <= keepHead + keepTail || p.length <= 40) return p

  const sep = p.includes('\\') ? '\\' : '/'
  const rooted = p.startsWith(sep)
  const head = (rooted ? sep : '') + parts.slice(0, keepHead).join(sep)
  const tail = parts.slice(-keepTail).join(sep)
  return `${head}${sep}…${sep}${tail}`
}

/** 单行显示时的兜底裁剪（中文按 1 字算，不按字节） */
export function clip(s: string, max: number): string {
  if (!s) return ''
  return s.length <= max ? s : `${s.slice(0, Math.max(1, max - 1))}…`
}
