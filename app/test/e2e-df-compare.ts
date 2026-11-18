/**
 * 体检第二步：按族去重后，最终方案会变成什么样。
 * 用缓存好的「谁」判定结果（e2e-who.json），不重调模型。
 */
import { readFileSync } from 'node:fs'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { buildProposal } from '../src/core/emergence/proposal'
import { buildFamilies } from '../src/core/version'
import type { FileItem } from '../src/core/types'

const raw = JSON.parse(readFileSync('test/desktop-scan.json', 'utf8'))
// desktop-scan.json 没有 id 字段，用路径顶上（buildProposal 靠 id 认文件）
const files = ((raw.files ?? raw) as FileItem[]).map((f) => ({ ...f, id: f.id ?? f.path }))
const who = new Set<string>(JSON.parse(readFileSync('test/e2e-who.json', 'utf8')).excluded)

const fams = buildFamilies(files.map((f) => ({ id: f.id, name: f.name, size: f.size, mtime: f.mtime })))
const oldIds = new Set<string>()
for (const fam of fams) for (const f of fam.files.slice(1)) oldIds.add(f.id)
const stats = files.filter((f) => !oldIds.has(f.id))

function run(label: string, forStats: FileItem[]) {
  const cands = collectCandidates(forStats.map((f) => ({ name: f.name, path: f.path })), { topK: 60 })
  const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
  const useful = cands.filter((c) => cn(c.term) >= 2)
  const groups = groupBySurface(useful, { who }).filter((g) => g.weight >= 3)
  const p = buildProposal(files, groups)
  const covered = p.topics.reduce((n, t) => n + t.fileIds.length, 0)
  console.log(`\n== ${label}`)
  console.log(`   事项 ${p.topics.length} / 已归类 ${covered} / 未归类 ${p.ungroupedIds.length} (${((p.ungroupedIds.length / files.length) * 100).toFixed(1)}%)`)
  console.log(`   兜底类 ${p.topics.filter((t) => t.weak).length}`)
  const top = [...p.topics].sort((a, b) => b.fileIds.length - a.fileIds.length).slice(0, 10)
  for (const t of top) console.log(`   ${String(t.fileIds.length).padStart(3)}  ${t.weak ? '[兜底]' : ''}${t.name}`)
  return p
}

const before = run('现状：所有版本都参与统计', files)
const after = run('按族去重：只有最新版参与统计', stats)

const names = new Set([...before.topics, ...after.topics].map((t) => t.name))
const onlyBefore = before.topics.filter((t) => !names.has(t.name) === false && !after.topics.some((x) => x.name === t.name))
const onlyAfter = after.topics.filter((t) => !before.topics.some((x) => x.name === t.name))
console.log(`\n  只在现状里有：${onlyBefore.map((t) => `${t.name}(${t.fileIds.length})`).join('、') || '无'}`)
console.log(`  只在去重后有：${onlyAfter.map((t) => `${t.name}(${t.fileIds.length})`).join('、') || '无'}`)
