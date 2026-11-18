/**
 * 真实数据探针：45 个事项 → 模型分域（第一层目录）
 *
 * 用法：npx tsx test/e2e-domain.ts [模型名]
 *
 * 走的是界面上完全一样的链路（提示词 / 解析 / 可用性判定都取自 core/emergence/domain.ts），
 * 唯一区别是这里直接 fetch Ollama，界面走 Rust 代理。
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  buildDomainPrompt,
  buildTree,
  domainsFromSharedToken,
  isUsable,
  parseDomainResult,
  type DomainTopic,
} from '../src/core/emergence/domain'

const here = dirname(fileURLToPath(import.meta.url))
const MODEL = process.argv[2] ?? 'qwen3.5:9b'

const saved = JSON.parse(readFileSync(resolve(here, 'e2e-proposal.json'), 'utf8')) as {
  proposal: { topics: { name: string; fileIds: string[]; weak?: boolean }[] }
}

// 兜底类（人名/单位/仅文种）不参与分域：它们本来就回答不了「哪件事」
const topics: DomainTopic[] = saved.proposal.topics
  .filter((t) => !t.weak)
  .map((t) => ({ name: t.name, weight: t.fileIds.length }))

const names = topics.map((t) => t.name)
const prompt = buildDomainPrompt(topics)
console.log(`正经事项 ${topics.length} 个 · 提示词 ${prompt.length} 字 · 送 ${MODEL} 分域…`)

const t0 = Date.now()
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
  signal: AbortSignal.timeout(300_000),
})
const json = (await res.json()) as {
  message?: { content?: string }
  total_duration?: number
  eval_count?: number
}
const raw = json.message?.content ?? ''
const secs = ((Date.now() - t0) / 1000).toFixed(1)

const r = parseDomainResult(raw, names)
const ok = isUsable(r, names)

console.log(`\n模型 ${secs}s · 输出 ${json.eval_count ?? '?'} token · 声明 ${r.domains.length} 个域 · 归类 ${r.assign.size}/${names.length}`)
console.log(`判定：${ok ? '可用' : '不可用（界面会退回规则分组）'}`)
console.log(`域：${r.domains.join(' / ')}`)

const tree = buildTree(
  topics.map((t) => ({ ...t, fileIds: new Array(t.weight).fill('x'), domain: r.assign.get(t.name) }))
)
const used = new Set<string>()
for (const d of tree.domains) {
  console.log(`\n■ ${d.name}  ${d.fileCount} 份 · ${d.topics.length} 个事项`)
  for (const t of d.topics) {
    used.add(t.name)
    console.log(`   · ${t.name}  ${t.fileIds.length} 份`)
  }
}
console.log(`\n一级零散事项 ${tree.loose.length} 个：${tree.loose.map((t) => t.name).join('、')}`)

const fb = domainsFromSharedToken(topics)
console.log(`\n规则兜底对照：${fb.domains.join(' / ')}（${fb.assign.size}/${names.length} 归入）`)

writeFileSync(
  resolve(here, 'e2e-domain.json'),
  JSON.stringify({ model: MODEL, secs, usable: ok, domains: r.domains, assign: Object.fromEntries(r.assign), raw }, null, 1),
  'utf-8'
)
