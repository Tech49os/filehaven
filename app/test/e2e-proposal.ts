/**
 * 端到端验证：真实扫描结果 → 涌现事项 → 提案 → 落库
 *
 * 用法：npx tsx test/e2e-proposal.ts [scan.json]
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { FileItem } from '../src/core/types'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'
import { buildProposal, reassign, applyProposal } from '../src/core/emergence/proposal'

const here = dirname(fileURLToPath(import.meta.url))
const path = process.argv[2] ?? resolve(here, 'desktop-scan.json')

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

const t0 = Date.now()
const cands = collectCandidates(
  files.map((f) => ({ name: f.name, path: f.path })),
  { topK: 60 }
)
const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
const useful = cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))
const groups = groupBySurface(useful).filter((g) => g.weight >= 3)
const proposal = buildProposal(files, groups)
const ms = Date.now() - t0

console.log(`\n扫描 ${files.length} 份 · 候选 ${cands.length} 个 · 事项 ${proposal.topics.length} 个 · 耗时 ${ms}ms`)
console.log(`已归类 ${files.length - proposal.ungroupedIds.length} 份，未归类 ${proposal.ungroupedIds.length} 份\n`)

const sorted = [...proposal.topics].sort((a, b) => b.fileIds.length - a.fileIds.length)
const nWeak = sorted.filter((t) => t.weak).length
console.log(`正经事项 ${sorted.length - nWeak} 个 · 仅文种兜底 ${nWeak} 个\n`)
sorted.forEach((t, i) => {
  const flag = t.weak ? ' [仅文种]' : ''
  console.log(`${String(i + 1).padStart(2)}. ${t.name.padEnd(14, ' ')} ${String(t.fileIds.length).padStart(3)} 份${flag}  ${t.samples[0]?.slice(0, 40) ?? ''}`)
})

// 模拟用户删掉两个最垃圾的类，验证文件回流
const junk = sorted.filter((t) => t.fileIds.length <= 2)
if (junk.length) {
  const after = reassign(files, proposal.topics.filter((t) => !junk.includes(t)))
  console.log(`\n[模拟] 删掉 ${junk.length} 个 ≤2 份的碎类后：`)
  console.log(`  事项 ${after.topics.length} 个，未归类 ${after.ungroupedIds.length} 份`)
}

const applied = applyProposal(files, proposal)
const pending = applied.files.filter((f) => f.needReview).length
console.log(`\n落库后：目录 ${applied.categories.length} 个，待人工指定 ${pending} 份`)
console.log(`路径示例：${applied.categories[0]?.name ?? '（无）'}\\${applied.files.find((f) => f.categoryId)?.name ?? ''}`)

writeFileSync(resolve(here, 'e2e-proposal.json'), JSON.stringify({ proposal, applied: { categories: applied.categories } }, null, 1), 'utf-8')
