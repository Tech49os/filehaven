/**
 * 真实桌面文件上的混合管线试跑（用 Rust 扫描产物，验证真实语料表现）
 *
 * 用法：npx tsx test/hybrid-real.ts <scan.json> [最多送模型份数] [模型]
 */
import { readFileSync } from 'node:fs'
import { induceHybrid } from '../src/core/hybrid'
import { induce } from '../src/core/classifier'
import type { FileItem } from '../src/core/types'

const path = process.argv[2] ?? 'test/desktop-scan.json'
const cap = Number(process.argv[3] ?? 20)
const model = process.argv[4] ?? 'qwen2.5:7b'
const conc = Number(process.argv[5] ?? 2)
/** 与 App 默认设置一致，否则统计出来的「待确认」数没有参考价值 */
const THRESHOLD = 0.85

interface RawFile {
  name: string
  path: string
  ext: string
  kind: string
  size: number
  mtime: number
  textSummary: string
  drawioNodes: string[]
}

const raw = JSON.parse(readFileSync(path, 'utf8')) as { files: RawFile[] }
const files: FileItem[] = raw.files.map((f, i) => ({
  id: `f${i}`,
  name: f.name,
  ext: f.ext,
  kind: f.kind as FileItem['kind'],
  size: f.size,
  mtime: f.mtime,
  path: f.path,
  textSummary: f.textSummary ?? '',
  drawioNodes: f.drawioNodes ?? [],
  categoryId: null,
  suggestedName: '',
  tags: [],
  confidence: 0,
  needReview: true,
  status: 'scanned',
}))

const withBody = files.filter(
  (f) => f.textSummary.trim().length > 30 || !!f.drawioNodes?.length
)
console.log(`\n扫描文件 ${files.length} 份，其中有正文 ${withBody.length} 份`)

const t0 = Date.now()
const rule = induce(files, { threshold: THRESHOLD })
const rulePending = rule.files.filter((f) => f.needReview).length
const ruleUnknown = rule.files.filter((f) => !f.ws).length
console.log(
  `规则初判：${Date.now() - t0}ms · 待确认 ${rulePending} · 判不出工作域 ${ruleUnknown}`
)

const t1 = Date.now()
const { files: out, stats } = await induceHybrid(files, {
  threshold: THRESHOLD,
  llm: {
    enabled: true,
    config: { baseUrl: 'http://127.0.0.1:11434', model, apiKey: '' },
    strategy: 'smart',
    concurrency: conc,
    maxFiles: cap,
  },
  onProgress: (p) => {
    if (p.phase === 'llm')
      process.stdout.write(`\r  模型复核 ${p.done}/${p.total}  `)
  },
})

console.log('\n=== 统计 ===')
console.log(JSON.stringify(stats, null, 2))

const after = out.filter((f) => f.needReview).length
const afterUnknown = out.filter((f) => !f.ws).length
const both = new Set(stats.model ? [stats.model] : [])
void both
console.log(
  `耗时 ${((Date.now() - t1) / 1000).toFixed(0)}s · 待确认 ${rulePending} → ${after} · ` +
    `判不出工作域 ${ruleUnknown} → ${afterUnknown}`
)

const shown = out.filter((f) => f.aiSource && f.aiSource !== 'rule' && f.aiSource !== 'none')
console.log(`\n=== 模型参与的 ${shown.length} 份 ===`)
const ruleMap = new Map(rule.files.map((f) => [f.id, f]))
for (const f of shown.slice(0, 25)) {
  const tag = { agree: '一致', llm: '补判', conflict: '分歧', none: '空', rule: '规则' }[
    f.aiSource ?? 'rule'
  ]
  const r = ruleMap.get(f.id)
  console.log(
    `[${tag}] ${f.name.slice(0, 38).padEnd(40)} 规则 ${(r?.ws ?? '空').padEnd(7)}${String(
      Math.round((r?.confidence ?? 0) * 100)
    ).padStart(4)}%  →  ${(f.ws ?? '空') + (f.dt ? ' / ' + f.dt : '')} (${Math.round(
      f.confidence * 100
    )}%)`
  )
  if (f.aiSource === 'conflict') console.log(`        ${f.reason}`)
}

const perFile = stats.llmDone ? (Date.now() - t1) / 1000 / stats.llmDone : 0
console.log(
  `\n单份实耗 ${perFile.toFixed(1)}s · 若全量复核 ${stats.llmCandidates} 份，约需 ${Math.round(
    (stats.llmCandidates * perFile) / 60
  )} 分钟`
)
if (stats.llmCandidates > cap) console.log(`本次已按上限 ${cap} 份截断`)
