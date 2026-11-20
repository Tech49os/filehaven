/**
 * 预览相关的小工具
 *
 * 图片不在后端解码（一张 4000px 的照片解出来再缩到 300px，纯浪费内存和 CPU），
 * 直接把本地路径交给 WebView：Tauri 的 asset 协议会当普通图片加载，按 CSS 缩放。
 *
 * 代价是要在 `tauri.conf.json` 里开 `app.security.assetProtocol`，并且
 * 纯浏览器里跑（没有 Tauri 上下文）时拿不到协议地址 —— 那种情况返回空串，
 * 调用方据此不渲染 <img>，而不是渲染一个坏图标。
 */

import { convertFileSrc } from '@tauri-apps/api/core'

export function hasTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 本地绝对路径 → 可在 <img src> 里用的地址；浏览器环境返回空串 */
export function imageSrc(absPath: string): string {
  if (!absPath || !hasTauri()) return ''
  return convertFileSrc(absPath)
}
