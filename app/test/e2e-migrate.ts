/**
 * 端到端验证（第二段）：真实扫描 → 提案 → 落库 → 迁移计划
 *
 * 前一段 e2e-proposal.ts 只验到「分类方案长什么样」，这一段验的是
 * 「方案确认之后，文件到底会被搬到哪、会不会撞名、会不会有文件被漏掉」。
 *
 * 用法：npx tsx test/e2e-migrate.ts [scan.json]
 */
import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { FileItem } from '../src/core/types'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'
import { buildProposal, applyProposal } from '../src/core/emergence/proposal'
import { buildPlans, sanitizeSegment } from '../src/core/plan'

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
  needReview: false,
  status: 'scanned',
}))

const cands = collectCandidates(
  files.map((f) => ({ name: f.name, path: f.path })),
  { topK: 60 }
)
const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
const useful = cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))
const groups = groupBySurface(useful).filter((g) => g.weight >= 3)
const proposal = buildProposal(files, groups)
const applied = applyProposal(files, proposal)
const plans = buildPlans(applied.files, applied.categories)

const active = plans.filter((p) => !p.deferred)
const deferred = plans.filter((p) => p.deferred)
const conflicts = active.filter((p) => p.conflict)
const dirs = new Set(active.map((p) => p.targetDir))

console.log(`扫描 ${files.length} 份 · 分类 ${applied.categories.length} 个 · 计划 ${plans.length} 条`)
console.log(`  会迁移 ${active.length} 份，暂不迁移 ${deferred.length} 份`)
console.log(`  目标目录 ${dirs.size} 个，撞名自动改名 ${conflicts.length} 份`)

// 1) 一份都不能丢
const covered = new Set(plans.map((p) => p.fileId))
const lost = files.filter((f) => !covered.has(f.id))
console.log(`\n[完整性] 计划覆盖 ${covered.size}/${files.length}${lost.length ? ` 缺 ${lost.length} 份！` : '，无遗漏'}`)

// 2) 同名必须让位，不能出现两条一样的 key
const keys = active.map((p) => `${p.targetDir}\\${p.targetName}`)
const dup = keys.filter((k, i) => keys.indexOf(k) !== i)
console.log(`[不覆盖] 目标路径重复 ${dup.length} 条${dup.length ? `：${dup.slice(0, 3).join(' / ')}` : '，全部唯一'}`)

// 3) 目录名必须已清洗（预览看到的 = 实际会建的）
const dirty = [...dirs].filter((d) => d.split('\\').some((s) => !s || sanitizeSegment(s) !== s))
console.log(`[目录名] 未清洗的 ${dirty.length} 个${dirty.length ? `：${dirty.slice(0, 3).join(' / ')}` : '，全部合法'}`)

// 4) 空目录不该被建出来
const emptyCats = applied.categories.filter(
  (c) => !applied.files.some((f) => f.categoryId === c.id)
)
console.log(`[空分类] ${emptyCats.length} 个${emptyCats.length ? `：${emptyCats.map((c) => c.name).join('、')}` : ''}`)

console.log('\n目标目录分布（前 16）：')
const byDir = new Map<string, number>()
for (const p of active) byDir.set(p.targetDir, (byDir.get(p.targetDir) ?? 0) + 1)
;[...byDir.entries()]
  .sort((a, b) => b[1] - a[1])
  .slice(0, 16)
  .forEach(([d, n], i) => console.log(`  ${String(i + 1).padStart(2)}. ${d.padEnd(22, ' ')} ${String(n).padStart(3)} 份`))

if (conflicts.length) {
  console.log('\n撞名改名示例：')
  for (const p of conflicts.slice(0, 5)) {
    console.log(`  ${p.fileName}  →  ${p.targetDir}\\${p.targetName}`)
  }
}
