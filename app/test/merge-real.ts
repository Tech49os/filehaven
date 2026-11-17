import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'
import { buildMergePrompt, parseMergeResult, fallbackMerge } from '../src/core/emergence/merge'

const here = dirname(fileURLToPath(import.meta.url))
const data = JSON.parse(readFileSync(resolve(here, 'real-classify.json'), 'utf-8'))
const files = data.details.map((d: any) => ({ name: d.name, path: d.rel }))

const MODEL = process.env.MODEL ?? 'qwen2.5:7b'
const cands = collectCandidates(files, { topK: 40 })
console.log(`文件 ${files.length} 份 → 候选 ${cands.length} 个`)

const prompt = buildMergePrompt(cands)
console.log(`prompt ${prompt.length} 字，送模型 ${MODEL}…`)
const t0 = Date.now()

let raw = ''
try {
  const res = await fetch('http://127.0.0.1:11434/api/chat', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model: MODEL,
      stream: false,
      keep_alive: '10m',
      format: 'json',
      messages: [{ role: 'user', content: prompt }],
      options: { temperature: 0, num_ctx: 4096, num_predict: 2000 },
    }),
  })
  const json: any = await res.json()
  raw = json?.message?.content ?? ''
} catch (e) {
  console.log('模型调用失败：', e instanceof Error ? e.message : e)
}

const secs = ((Date.now() - t0) / 1000).toFixed(1)

if (!raw) {
  console.log(`\n模型不可用（${secs}s），走降级方案：`)
  const fb = fallbackMerge(cands, 12)
  fb.forEach((t, i) => console.log(`${i + 1}. ${t.name}`))
} else {
  writeFileSync(resolve(here, 'merge-raw.json'), raw, 'utf-8')
  try {
    const r = parseMergeResult(raw, cands)
    console.log(`\n用时 ${secs}s · 模型给出 ${r.topics.length} 个事项，${r.noise.length} 个判为噪音`)
    if (r.rejected.length) console.log(`模型编造并被剔除的词：${r.rejected.join('、')}`)
    console.log('\n=== 事项 ===')
    r.topics.forEach((t, i) => {
      console.log(`${String(i + 1).padStart(2)}. ${t.name}  （${t.aliases.join('、')}）`)
      if (t.desc) console.log(`    ${t.desc}`)
    })
    console.log('\n=== 判为噪音 ===')
    console.log(r.noise.join('、'))
    writeFileSync(resolve(here, 'merge-real.json'), JSON.stringify(r, null, 1), 'utf-8')
  } catch (e) {
    console.log(`\n解析失败（${secs}s）：`, e instanceof Error ? e.message : e)
    console.log('原始输出前 600 字：\n', raw.slice(0, 600))
  }
}
