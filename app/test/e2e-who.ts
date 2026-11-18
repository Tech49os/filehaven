/**
 * 端到端：真实 455 份 → 规则抽取候选 → 模型判「谁」→ 分组 → 提案
 *
 * 走的是和界面完全一样的链路（唯一的区别是这里直接 fetch，界面走 Rust 代理）。
 *
 * 用法：npx tsx test/e2e-who.ts [模型名]
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'
import { buildProposal } from '../src/core/emergence/proposal'
import {
  augmentWithOrgRule,
  buildWhoPrompt,
  expandWho,
  parseWhoResult,
} from '../src/core/emergence/whoJudge'
import type { FileItem } from '../src/core/types'

const here = dirname(fileURLToPath(import.meta.url))
const MODEL = process.argv[2] ?? 'qwen3.5:9b'

const raw = JSON.parse(readFileSync(resolve(here, 'desktop-scan.json'), 'utf8')) as {
  files: { name: string; path: string; ext: string; kind: string; size: number; mtime: number; textSummary: string; drawioNodes: string[] }[]
}
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

const t0 = Date.now()
const cands = collectCandidates(files.map((f) => ({ name: f.name, path: f.path })), { topK: 60 })
const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
const useful = cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))
const tRule = Date.now()

const prompt = buildWhoPrompt(useful)
console.log(`候选 ${useful.length} 个 · 规则耗时 ${tRule - t0}ms · 送模型 ${MODEL} 判「谁」…`)

const res = await fetch('http://127.0.0.1:11434/api/chat', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    model: MODEL,
    stream: false,
    keep_alive: '10m',
    format: 'json',
    think: false,
    messages: [{ role: 'user', content: prompt }],
    options: { temperature: 0, num_ctx: 4096, num_predict: 2048 },
  }),
  signal: AbortSignal.timeout(120_000),
})
const json = (await res.json()) as { message?: { content?: string } }
const base = parseWhoResult(json.message?.content ?? '', useful)
const who = expandWho(augmentWithOrgRule(base, useful), useful)
const tModel = Date.now()

const groups = groupBySurface(useful, { who }).filter((g) => g.weight >= 3)
const proposal = buildProposal(files, groups)
const excluded = useful.filter((c) => who.has(c.term)).map((c) => c.term)

console.log(
  `模型 ${((tModel - tRule) / 1000).toFixed(1)}s · 判为「谁」${who.size} 个 · 事项 ${proposal.topics.length} 个 · 总耗时 ${Date.now() - t0}ms`
)
console.log(`已归类 ${files.length - proposal.ungroupedIds.length} 份，未归类 ${proposal.ungroupedIds.length} 份\n`)

const sorted = [...proposal.topics].sort((a, b) => b.fileIds.length - a.fileIds.length)
const nWeak = sorted.filter((t) => t.weak).length
console.log(`正经事项 ${sorted.length - nWeak} 个 · 仅文种兜底 ${nWeak} 个\n`)
sorted.forEach((t, i) => {
  console.log(
    `${String(i + 1).padStart(2)}. ${t.name.padEnd(14, ' ')} ${String(t.fileIds.length).padStart(3)} 份${t.weak ? ' [兜底]' : ''}`
  )
})
console.log(`\n判为人名/单位（折叠为兜底类）：${excluded.join('、')}`)

writeFileSync(
  resolve(here, 'e2e-who.json'),
  JSON.stringify({ excluded, topics: sorted.map((t) => ({ name: t.name, n: t.fileIds.length, weak: t.weak })) }, null, 1),
  'utf-8'
)
