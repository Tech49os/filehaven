/**
 * 9b 长 prompt 归并压力对照实验
 * 用真实 candidates-real.json + 真实 buildMergePrompt，只改 num_ctx / num_predict
 *
 * npx tsx test/stress-merge.ts --n 40 --num_ctx 16384 --num_predict 3000
 */
import { readFileSync } from 'node:fs'
import { buildMergePrompt, parseMergeResult } from '../src/core/emergence/merge'
import type { Candidate } from '../src/core/emergence/candidates'

const args = process.argv.slice(2)
const get = (k: string, d: string) => {
  const i = args.indexOf('--' + k)
  return i >= 0 ? args[i + 1] : d
}

const N = Number(get('n', '40'))
const NUM_CTX = Number(get('num_ctx', '4096'))
const NUM_PREDICT = Number(get('num_predict', '3000'))
const THINK = get('think', 'false') === 'true'
const MODEL = get('model', 'qwen3.5:9b')

const all = JSON.parse(readFileSync(new URL('./candidates-real.json', import.meta.url), 'utf8')) as Candidate[]
const cands = all.slice(0, N)
const prompt = buildMergePrompt(cands)

const estTok = Math.round(prompt.length / 1.55)
console.log(`[${MODEL}] n=${N} prompt=${prompt.length}字 ≈${estTok}tok  num_ctx=${NUM_CTX} num_predict=${NUM_PREDICT} think=${THINK}`)

const t0 = Date.now()
let ttft = 0
let raw = ''
try {
  const res = await fetch('http://127.0.0.1:11434/api/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model: MODEL,
      prompt,
      stream: true,
      think: THINK,
      options: { temperature: 0.2, num_ctx: NUM_CTX, num_predict: NUM_PREDICT },
    }),
  })
  if (!res.body) throw new Error('no body')
  const reader = res.body.getReader()
  const dec = new TextDecoder()
  let buf = ''
  let final: any = null
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    buf += dec.decode(value, { stream: true })
    let idx
    while ((idx = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, idx).trim()
      buf = buf.slice(idx + 1)
      if (!line) continue
      const obj = JSON.parse(line)
      if (obj.response) {
        if (!ttft) ttft = Date.now() - t0
        raw += obj.response
      }
      if (obj.done) final = obj
    }
  }
  const el = (Date.now() - t0) / 1000
  console.log(`完成 ${el.toFixed(1)}s / 首字 ${(ttft / 1000).toFixed(1)}s`)
  if (final) {
    console.log(
      `done_reason=${final.done_reason} prompt_eval=${final.prompt_eval_count}tok(${(
        (final.prompt_eval_duration || 0) / 1e9
      ).toFixed(1)}s) eval=${final.eval_count}tok(${((final.eval_duration || 0) / 1e9).toFixed(1)}s)`
    )
    if (final.eval_count) {
      console.log(`生成速度 ${(final.eval_count / ((final.eval_duration || 1) / 1e9)).toFixed(2)} tok/s`)
    }
  }
  console.log(`输出 ${raw.length} 字，尾部: ${JSON.stringify(raw.slice(-160))}`)
  try {
    const r = parseMergeResult(raw, cands)
    const covered = r.topics.reduce((s, t) => s + t.aliases.length, 0)
    console.log(
      `解析成功: ${r.topics.length} 组 / 覆盖 ${covered}/${cands.length} / noise=${r.noise.length} / rejected=${r.rejected.length}`
    )
    console.log('组名: ' + r.topics.map((t) => `${t.name}(${t.aliases.length})`).join(' | '))
  } catch (e: any) {
    console.log(`解析失败: ${e.message}`)
  }
} catch (e: any) {
  console.log(`请求异常 ${(Date.now() - t0) / 1000}s: ${e.message}`)
  console.log(`已收 ${raw.length} 字，尾部: ${JSON.stringify(raw.slice(-200))}`)
}
