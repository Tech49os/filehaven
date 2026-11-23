/**
 * 界面偏好（BUG-011）：材质效果开关。
 *
 * 默认开（用户裁决：全局苹果风重做）；低性能设备可在设置页关掉，
 * 关掉后整页加 html.no-glass 完整回退实色。偏好写 localStorage，
 * 重启后仍按用户的意愿 —— 纯前端偏好不需要走 Tauri 设置表，
 * 也让纯 Vite dev / 浏览器 CDP 量测能在同一套代码上验证。
 */
import { ref } from 'vue'
import { defineStore } from 'pinia'

const KEY = 'filebutler.glassEnabled'

function readGlass(): boolean {
  try {
    return globalThis.localStorage?.getItem(KEY) !== '0'
  } catch {
    return true
  }
}

export const useUiStore = defineStore('ui', () => {
  const glassEnabled = ref(readGlass())

  function setGlassEnabled(on: boolean) {
    glassEnabled.value = on
    try {
      globalThis.localStorage?.setItem(KEY, on ? '1' : '0')
    } catch {
      /* 隐私模式下写不进去也要让本次会话生效 */
    }
  }

  return { glassEnabled, setGlassEnabled }
})
