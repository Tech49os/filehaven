import { segments } from './src/core/emergence/normalize'
const seg = '智能工厂技术室'
const chars = [...seg]
const L = chars.length
console.log('L=', L)
const out: string[] = []
if (L >= 2 && L <= 12) out.push(seg)
if (L > 4) {
  for (let n = 3; n <= Math.min(8, L - 1); n++) {
    for (let i = 0; i + n <= L; i++) out.push(chars.slice(i, i + n).join(''))
  }
}
console.log('forms:', out.join(' | '))
console.log('含整段?', out.includes('智能工厂技术室'))
