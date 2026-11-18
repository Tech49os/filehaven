/**
 * 沿用存档的端到端验证：
 *   1. 首次生成 → 存盘
 *   2. 用存档重新认领同一批文件 → 应得到完全相同的归类结果
 *   3. 打乱文件顺序（模拟不同扫描顺序）→ 结果仍应一致
 */
import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { buildProposal } from '../src/core/emergence/proposal'
import { toSavedPlan, buildProposalFromSaved } from '../src/core/emergence/archive'
import { sortForDisplay } from '../src/core/emergence/view'
import type { FileItem } from '../src/core/types'

const here = dirname(fileURLToPath(import.meta.url))
const raw = JSON.parse(readFileSync(resolve(here, 'desktop-scan.json'), 'utf8')) as {
  files: { name: string; path: string }[]
}

const files: FileItem[] = raw.files.map((f, i) => ({
  id: `f${i}`,
  name: f.name,
  ext: (f.name.match(/\.[^.]+$/) ?? [''])[0],
  kind: 'doc',
  size: 1024,
  mtime: 0,
  path: f.path,
  textSummary: '',
  categoryId: null,
  suggestedName: '',
  tags: [],
  confidence: 0,
  needReview: true,
  status: 'scanned',
})) as FileItem[]

// ① 首次生成
const t0 = Date.now()
const cands = collectCandidates(files.map((f) => ({ name: f.name, path: f.path })), { topK: 60 })
const groups = groupBySurface(cands).filter((g) => g.weight >= 3)
const first = buildProposal(files, groups)
const tFirst = Date.now() - t0

// ② 存盘 + 沿用
const saved = toSavedPlan(first, 'D:\\文档归档')
const t1 = Date.now()
const reused = buildProposalFromSaved(files, saved)!
const tReuse = Date.now() - t1

const sig = (p: typeof first) =>
  sortForDisplay(p.topics)
    .map((t) => `${t.name}:${[...t.fileIds].sort().join(',')}`)
    .join('|')

console.log(`首次生成 ${first.topics.length} 类 / 归类 ${files.length - first.ungroupedIds.length} 份 / ${tFirst}ms`)
console.log(`沿用存档 ${reused.topics.length} 类 / 归类 ${files.length - reused.ungroupedIds.length} 份 / ${tReuse}ms`)
console.log(`归类结果完全一致：${sig(first) === sig(reused) ? '是' : '否（有差异）'}`)

// ③ 打乱顺序再沿用
const shuffled = [...files].sort(() => Math.random() - 0.5)
const reused2 = buildProposalFromSaved(shuffled, saved)!
const sigOf = (p: typeof first, src: FileItem[]) => {
  const nameById = new Map(src.map((f) => [f.id, f.name]))
  return sortForDisplay(p.topics)
    .map((t) => `${t.name}:${t.fileIds.map((id) => nameById.get(id)).sort().join(',')}`)
    .join('|')
}
console.log(`打乱扫描顺序后结果仍一致：${sigOf(reused, files) === sigOf(reused2, shuffled) ? '是' : '否'}`)

// ④ 存档体积
console.log(`存档体积 ${(JSON.stringify(saved).length / 1024).toFixed(1)} KB（${saved.topics.length} 个类名 + 别名）`)
