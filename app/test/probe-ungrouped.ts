import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { buildProposal } from '../src/core/emergence/proposal'
import type { FileItem } from '../src/core/types'

const here = dirname(fileURLToPath(import.meta.url))
const raw = JSON.parse(readFileSync(resolve(here, 'desktop-scan.json'), 'utf8')) as {
  files: { name: string; path: string }[]
}
const files = raw.files.map((f, i) => ({
  id: `f${i}`, name: f.name, ext: '', kind: 'doc', size: 1, mtime: 0, path: f.path,
  textSummary: '', categoryId: null, suggestedName: '', tags: [], confidence: 0,
  needReview: true, status: 'scanned',
})) as unknown as FileItem[]

const cands = collectCandidates(files.map((f) => ({ name: f.name, path: f.path })), { topK: 60 })
const groups = groupBySurface(cands).filter((g) => g.weight >= 3)
const p = buildProposal(files, groups)

const byId = new Map(files.map((f) => [f.id, f]))
const folder = (path: string) => {
  const i = path.lastIndexOf('\\')
  return i < 0 ? '(根目录)' : path.slice(0, i)
}
const counter = new Map<string, number>()
for (const id of p.ungroupedIds) {
  const f = byId.get(id)!
  const d = folder(f.path)
  counter.set(d, (counter.get(d) ?? 0) + 1)
}
const rows = [...counter.entries()].sort((a, b) => b[1] - a[1])
console.log(`未归类 ${p.ungroupedIds.length} 份，分布在 ${rows.length} 个目录`)
for (const [d, n] of rows.slice(0, 12)) console.log(String(n).padStart(3), d)
console.log('\n样例文件名：')
for (const id of p.ungroupedIds.slice(0, 12)) console.log('  ', byId.get(id)!.name.slice(0, 60))
