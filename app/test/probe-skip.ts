/**
 * 「扫描时跳过非文档」的效果验证。
 * 链路要与 stores/wizard.ts 的 runProposal 一致：纯英文短词/文种词剔除 + 组织名降级。
 * （不含模型判谁：离线跑，用规则版 augmentWithOrgRule 代替，两组数据同口径。）
 */
import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isNonDocument } from '../src/core/skip'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { buildProposal } from '../src/core/emergence/proposal'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'
import { augmentWithOrgRule, expandWho } from '../src/core/emergence/whoJudge'
import { sortForDisplay } from '../src/core/emergence/view'
import type { FileItem } from '../src/core/types'

const here = dirname(fileURLToPath(import.meta.url))
const raw = JSON.parse(readFileSync(resolve(here, 'desktop-scan.json'), 'utf8')) as {
  files: { name: string; path: string }[]
}

const toItems = (list: typeof raw.files) =>
  list.map((f, i) => ({
    id: `f${i}`, name: f.name, ext: '', kind: 'doc', size: 1, mtime: 0, path: f.path,
    textSummary: '', categoryId: null, suggestedName: '', tags: [], confidence: 0,
    needReview: true, status: 'scanned',
  })) as unknown as FileItem[]

function pipeline(list: typeof raw.files) {
  const files = toItems(list)
  const cands = collectCandidates(list.map((f) => ({ name: f.name, path: f.path })), { topK: 60 })
  const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
  const useful = cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))
  const who = expandWho(augmentWithOrgRule(new Map(), useful), useful)
  const groups = groupBySurface(useful, { who }).filter((g) => g.weight >= 3)
  const p = buildProposal(files, groups)
  return { files, p, topics: sortForDisplay(p.topics) }
}

const kept = raw.files.filter((f) => !isNonDocument(f.name))
const dropped = raw.files.filter((f) => isNonDocument(f.name))

const before = pipeline(raw.files)
const after = pipeline(kept)

const show = (label: string, r: ReturnType<typeof pipeline>) => {
  const assigned = r.files.length - r.p.ungroupedIds.length
  console.log(
    `${label}：${r.files.length} 份 · ${r.topics.length} 类 · 归类 ${assigned} / 未归类 ${r.p.ungroupedIds.length}（${((r.p.ungroupedIds.length / r.files.length) * 100).toFixed(0)}%）`
  )
}

show('过滤前', before)
show('过滤后', after)
console.log(`跳过 ${dropped.length} 个非文档：`)
const byExt = new Map<string, number>()
for (const f of dropped) {
  const e = (f.name.match(/\.[^.]+$/) ?? ['(无)'])[0].toLowerCase()
  byExt.set(e, (byExt.get(e) ?? 0) + 1)
}
console.log('   ' + [...byExt.entries()].sort((a, b) => b[1] - a[1]).map(([e, n]) => `${e}×${n}`).join('  '))

const a = new Map(before.topics.map((t) => [t.name, t.fileIds.length]))
const b = new Map(after.topics.map((t) => [t.name, t.fileIds.length]))
const diffs = [...new Set([...a.keys(), ...b.keys()])]
  .filter((n) => (a.get(n) ?? 0) !== (b.get(n) ?? 0))
  .map((n) => `${n} ${a.get(n) ?? 0}→${b.get(n) ?? 0}`)
console.log(`\n类目变化 ${diffs.length} 处：${diffs.slice(0, 14).join(' · ')}`)
