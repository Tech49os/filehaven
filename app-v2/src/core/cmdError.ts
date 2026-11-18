/**
 * 命令错误的结构化封装（详设 §6.1）。
 *
 * Rust 侧统一 `Err(CmdError { code, message })`，Tauri 会把它原样 reject，
 * 所以前端可以按 `code` 分支，绝不去匹配中文文案。
 */

export interface CmdError {
  code: string
  message: string
}

/** 错误码常量：与 `src-tauri/src/error.rs` 一一对应 */
export const ERR = {
  MODEL_DOWN: 'MODEL_DOWN',
  MODEL_TIMEOUT: 'MODEL_TIMEOUT',
  MODEL_BAD_OUTPUT: 'MODEL_BAD_OUTPUT',
  /** 文件太少（<10 份），不请求模型 */
  TOO_FEW_FILES: 'TOO_FEW_FILES',
  BAD_TRANSITION: 'BAD_TRANSITION',
  DIR_EXISTS: 'DIR_EXISTS',
  DIR_INVALID: 'DIR_INVALID',
  ARCHIVE_IN_SOURCE: 'ARCHIVE_IN_SOURCE',
  SOURCE_UNREADABLE: 'SOURCE_UNREADABLE',
  FILE_CHANGED: 'FILE_CHANGED',
  MIGRATE_FAILED: 'MIGRATE_FAILED',
  DB: 'DB',
  IO: 'IO',
  NOT_CONFIGURED: 'NOT_CONFIGURED',
  /** 后端还没这个命令（M5/M7 未接入时的降级判据） */
  UNKNOWN_CMD: 'UNKNOWN_CMD',
} as const

/** 把任意 reject 值收敛成 { code, message } */
export function toCmdError(e: unknown): CmdError {
  if (typeof e === 'object' && e !== null) {
    const o = e as Record<string, unknown>
    if (typeof o.code === 'string' && typeof o.message === 'string') {
      return { code: o.code, message: o.message }
    }
    if (typeof o.message === 'string') {
      return { code: guessCode(o.message), message: o.message }
    }
  }
  if (typeof e === 'string') return { code: guessCode(e), message: e }
  return { code: ERR.IO, message: '未知错误' }
}

/**
 * Tauri 对「命令不存在」只在 message 里说，没给码。
 * 这是唯一允许看文案的地方，且只为降级判断用。
 */
function guessCode(message: string): string {
  if (/not found|No handler|unknown command/i.test(message)) return ERR.UNKNOWN_CMD
  return ERR.IO
}

/** 给用户看的一句话（按 code 优先，兜底服务端 message） */
export function humanize(e: CmdError): string {
  switch (e.code) {
    case ERR.ARCHIVE_IN_SOURCE:
      return '归档区不能放在源区里'
    case ERR.DIR_EXISTS:
      return '这个目录已经有了'
    case ERR.DIR_INVALID:
      return e.message || '目录名不可用'
    case ERR.NOT_CONFIGURED:
      return '还没有选归档区'
    case ERR.SOURCE_UNREADABLE:
      return '源区不可读，可能被删掉了或没有权限'
    case ERR.BAD_TRANSITION:
      return '这个操作和当前状态冲突，刷新一下再试'
    case ERR.MODEL_DOWN:
      return '本地模型不可达，可以先手动建目录'
    case ERR.MODEL_TIMEOUT:
      return '模型响应超时，可以先手动建目录'
    case ERR.MODEL_BAD_OUTPUT:
      return '模型这次没给出可用的方案'
    case ERR.TOO_FEW_FILES:
      return e.message || '文件太少了，先多扫一些再归纳'
    case ERR.FILE_CHANGED:
      return '文件在标注后被改动过'
    default:
      return e.message
  }
}
