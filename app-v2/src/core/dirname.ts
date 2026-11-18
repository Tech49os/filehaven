/**
 * 目录名清洗（与 Rust `src-tauri/src/plan.rs` 逐字对应）
 *
 * 评审红线 2：任何一侧改规则必须同时改 `test/fixtures/dirname-cases.json`，
 * 两侧共跑同一份用例，任一侧红即阻断。
 */

export interface DirNamePreview {
  cleaned: string
  valid: boolean
  reason: string | null
}

/** 1. 非法字符 \ / : * ? " < > | 与控制字符 → _ */
const ILLEGAL = /[\\/:*?"<>|\u0000-\u001f]/g

const RESERVED_FIXED = ['CON', 'PRN', 'AUX', 'NUL']

function isReserved(upperStem: string): boolean {
  if (RESERVED_FIXED.includes(upperStem)) return true
  return /^(COM|LPT)[1-9]$/.test(upperStem)
}

// ---------------------------------------------------------------- md5
// 只为一个用途：超长目录名的 8 位哈希后缀。
// 不引第三方包（P1 不装 npm 依赖），也不依赖 node:crypto（浏览器里没有），
// 所以这里放一份标准 MD5。正确性由 dirname-cases.json 的第 15 条兜住。

function rotl(x: number, n: number): number {
  return ((x << n) | (x >>> (32 - n))) >>> 0
}

const MD5_S = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21]
const MD5_K = (() => {
  const k = new Uint32Array(64)
  for (let i = 0; i < 64; i++) k[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 4294967296) >>> 0
  return k
})()

export function md5Hex(input: string): string {
  const bytes = new TextEncoder().encode(input)
  const len = bytes.length
  const total = (((len + 8) >> 6) + 1) * 64
  const buf = new Uint8Array(total)
  buf.set(bytes)
  buf[len] = 0x80
  const dv = new DataView(buf.buffer)
  const bitLen = len * 8
  dv.setUint32(total - 8, bitLen >>> 0, true)
  dv.setUint32(total - 4, Math.floor(bitLen / 4294967296), true)

  let a0 = 0x67452301
  let b0 = 0xefcdab89
  let c0 = 0x98badcfe
  let d0 = 0x10325476
  const m = new Uint32Array(16)

  for (let off = 0; off < total; off += 64) {
    for (let i = 0; i < 16; i++) m[i] = dv.getUint32(off + i * 4, true)
    let a = a0
    let b = b0
    let c = c0
    let d = d0
    for (let i = 0; i < 64; i++) {
      let f: number
      let g: number
      if (i < 16) {
        f = (b & c) | (~b & d)
        g = i
      } else if (i < 32) {
        f = (d & b) | (~d & c)
        g = (5 * i + 1) % 16
      } else if (i < 48) {
        f = b ^ c ^ d
        g = (3 * i + 5) % 16
      } else {
        f = c ^ (b | ~d)
        g = (7 * i) % 16
      }
      f = (f + a + MD5_K[i] + m[g]) >>> 0
      a = d
      d = c
      c = b
      b = (b + rotl(f, MD5_S[(i >> 4) * 4 + (i % 4)])) >>> 0
    }
    a0 = (a0 + a) >>> 0
    b0 = (b0 + b) >>> 0
    c0 = (c0 + c) >>> 0
    d0 = (d0 + d) >>> 0
  }

  const hex = (w: number) => {
    let s = ''
    for (let i = 0; i < 4; i++) s += ((w >>> (i * 8)) & 0xff).toString(16).padStart(2, '0')
    return s
  }
  return hex(a0) + hex(b0) + hex(c0) + hex(d0)
}

// ---------------------------------------------------------------- 清洗

/** 单段目录名清洗（不含 `/`，路径拆分在 cleanPath 里做） */
export function cleanDirName(input: string): DirNamePreview {
  // 1. 非法字符 → _
  let s = input.replace(ILLEGAL, '_')

  // 2. 首尾空格 + 尾部点号循环去除 + 首部点号去除
  s = s.trim()
  while (s.endsWith('.')) s = s.slice(0, -1)
  while (s.startsWith('.')) s = s.slice(1)
  s = s.trim()

  // 3. 连续 _ 折叠
  s = s.replace(/_+/g, '_')

  if (s.length === 0) {
    return { cleaned: '', valid: false, reason: '目录名不能为空' }
  }

  // 4. 超长截断：前 51 字符 + _ + md5 前 8 位
  if ([...s].length > 60) {
    const head = [...s].slice(0, 51).join('')
    s = `${head}_${md5Hex(s).slice(0, 8)}`
  }

  // 5. Windows 保留名 → 前置 _
  const dot = s.lastIndexOf('.')
  const stem = dot > 0 ? s.slice(0, dot) : s
  if (isReserved(stem.toUpperCase())) s = `_${s}`

  return { cleaned: s, valid: true, reason: null }
}

/** 多段相对路径：`父/子` 逐段清洗后用 `/` 拼回 */
export function cleanPath(input: string): DirNamePreview {
  const parts: string[] = []
  for (const seg of input.replace(/\\/g, '/').split('/')) {
    if (seg.trim().length === 0) continue
    const p = cleanDirName(seg)
    if (!p.valid) return p
    parts.push(p.cleaned)
  }
  if (parts.length === 0) {
    return { cleaned: '', valid: false, reason: '目录名不能为空' }
  }
  return { cleaned: parts.join('/'), valid: true, reason: null }
}
