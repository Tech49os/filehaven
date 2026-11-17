/**
 * 诊断：39 个提案事项里，哪些是「真事项」，哪些是碎片/背景词。
 *
 * 只用两个可计算指标，不引入任何行业词：
 *   soloDf   —— 文件名里只命中这一个事项（没有同时命中别的事项）的份数
 *   wholeDf  —— 这个词作为**完整段**出现过的份数（区别于只是长名字的一截）
 *
 * 用法：npx tsx test/analyze-topic-quality.ts
 */
import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { FileItem } from '../src/core/types'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'
import { buildProposal } from '../src/core/emergence/proposal'
import { segments } from '../src/core/emergence/normalize'

const here = dirname(fileURLToPath(import.meta.url))
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

const cands = collectCandidates(files.map((f) => ({ name: f.name, path: f.path })), { topK: 60 })
const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
const useful = cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))
const groups = groupBySurface(useful).filter((g) => g.weight >= 3)
const proposal = buildProposal(files, groups)

const byId = new Map(files.map((f) => [f.id, f]))
const topics = [...proposal.topics].sort((a, b) => b.fileIds.length - a.fileIds.length)

console.log(
  '\n序号 名称              总份 独占 整段  独占率 整段率  判断线索'
)
console.log('─'.repeat(96))

for (const [i, t] of topics.entries()) {
  const own = t.fileIds.map((id) => byId.get(id)!).filter(Boolean)
  const others = topics.filter((o) => o.id !== t.id)

  let solo = 0
  for (const f of own) {
    const hitOthers = others.some((o) =>
      o.aliases.some((a) => a.length >= 3 && f.name.includes(a))
    )
    if (!hitOthers) solo++
  }

  let whole = 0
  for (const f of own) {
    const segs = segments(f.name)
    if (segs.some((s) => s === t.name)) whole++
    else if (segs.some((s) => s.startsWith(t.name) || s.endsWith(t.name))) whole++
  }

  const soloRate = own.length ? solo / own.length : 0
  const wholeRate = own.length ? whole / own.length : 0
  const hint =
    wholeRate === 0
      ? '从不成段 → 碎片'
      : soloRate < 0.35
        ? '几乎总是和别的类同时出现 → 背景词'
        : ''

  console.log(
    `${String(i + 1).padStart(2)}. ${t.name.padEnd(16, '　')} ${String(own.length).padStart(3)} ${String(solo).padStart(4)} ${String(whole).padStart(4)}  ${(soloRate * 100).toFixed(0).padStart(4)}%  ${(wholeRate * 100).toFixed(0).padStart(4)}%  ${hint}`
  )
}
