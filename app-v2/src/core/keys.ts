/**
 * 键盘映射（详设 §8.3）
 *
 * 唯一的坑：输入框打开时除了 Esc / Enter 一律不拦，
 * 否则在「新建目录」输入框里打不出 s / p / n —— 单测覆盖了这一条。
 */

export type KeyCmd =
  | { t: 'accept' }
  | { t: 'pick'; n: 1 | 2 | 3 }
  | { t: 'newDir' }
  | { t: 'defer' }
  | { t: 'park' }
  | { t: 'move'; d: 1 | -1 }
  | { t: 'escape' }
  | { t: 'undo' }
  /** 打开当前文件（系统默认程序） */
  | { t: 'open' }
  /** 在资源管理器中定位当前文件 */
  | { t: 'reveal' }

export interface KeyEventLike {
  key: string
  ctrlKey: boolean
  metaKey: boolean
}

export interface KeyCtx {
  /** 有输入框获得焦点 */
  inputOpen: boolean
}

export function mapKey(e: KeyEventLike, ctx: KeyCtx): KeyCmd | null {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
    // 输入框里的 Ctrl+Z 是撤销文字，不能抢
    return ctx.inputOpen ? null : { t: 'undo' }
  }

  if (e.key === 'Escape') return { t: 'escape' }

  if (ctx.inputOpen) {
    // 输入框里只认「提交」和「关闭」，其余交给浏览器
    return e.key === 'Enter' ? { t: 'accept' } : null
  }

  if (e.ctrlKey || e.metaKey) return null

  switch (e.key) {
    case 'Enter':
      return { t: 'accept' }
    case '1':
      return { t: 'pick', n: 1 }
    case '2':
      return { t: 'pick', n: 2 }
    case '3':
      return { t: 'pick', n: 3 }
    case 'ArrowDown':
      return { t: 'move', d: 1 }
    case 'ArrowUp':
      return { t: 'move', d: -1 }
  }

  switch (e.key.toLowerCase()) {
    case 'n':
      return { t: 'newDir' }
    case 's':
      return { t: 'defer' }
    case 'p':
      return { t: 'park' }
    // 判一份文件常要「打开确认一眼再回来」，来回摸鼠标会打断流水线
    case 'o':
      return { t: 'open' }
    case 'l':
      return { t: 'reveal' }
  }
  return null
}
