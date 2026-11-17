/**
 * 分类引擎离线校验
 * 用法：npx tsx test/validate.ts
 *
 * 校验三件事：
 *  1. 一级「工作域」判得准不准（这是归档正确性的关键——归错项目就找不回来了）
 *  2. 二级「文档类型」判得准不准（错了不致命，文件还在正确项目里）
 *  3. 拿不准的文件能否被诚实推进待确认队列
 */
import { ALL_SAMPLES } from '../src/core/sample'
import { induce } from '../src/core/classifier'
import { extOf } from '../src/core/extractor'
import type { FileItem } from '../src/core/types'

const now = Date.now()

function toFileItem(s: (typeof ALL_SAMPLES)[number], i: number): FileItem {
  return {
    id: `f${i}`,
    name: s.name,
    ext: extOf(s.name),
    kind: s.kind,
    size: s.size * 1024,
    mtime: now - s.daysAgo * 86400000,
    path: `D:\\混乱目录\\${s.name}`,
    textSummary: s.content,
    drawioNodes: s.drawioNodes,
    categoryId: null,
    suggestedName: '',
    tags: [],
    confidence: 0,
    needReview: true,
    status: 'scanned',
  }
}

const items = ALL_SAMPLES.map(toFileItem)
const { result, files } = induce(items, { threshold: 0.7 })

const catById = new Map(result.categories.map((c) => [c.id, c]))

function pathOf(f: FileItem): { ws: string | null; dt: string | null } {
  if (!f.categoryId) return { ws: null, dt: null }
  const c = catById.get(f.categoryId)
  if (!c) return { ws: null, dt: null }
  if (c.parentId) return { ws: catById.get(c.parentId)?.name ?? null, dt: c.name }
  return { ws: c.name, dt: null }
}

let wsTotal = 0, wsHit = 0
let dtTotal = 0, dtHit = 0
let hardTotal = 0, hardHit = 0
let autoPass = 0
const perWs = new Map<string, { total: number; hit: number }>()
const misses: string[] = []

files.forEach((f, i) => {
  const s = ALL_SAMPLES[i]

  // 难例：应当被推进待确认
  if (s.ws === null) {
    hardTotal++
    if (f.needReview) hardHit++
    else misses.push(`[难例漏过] ${f.name} → ${pathOf(f).ws}/${pathOf(f).dt} ${(f.confidence * 100).toFixed(0)}%`)
    return
  }

  // 清晰样本：只统计"自动通过"的部分，因为待确认的会由人工纠正
  if (f.needReview) return
  autoPass++

  const p = pathOf(f)
  wsTotal++
  if (!perWs.has(s.ws)) perWs.set(s.ws, { total: 0, hit: 0 })
  const rec = perWs.get(s.ws)!
  rec.total++

  if (p.ws === s.ws) {
    wsHit++
    rec.hit++
  } else {
    misses.push(`[工作域错] ${f.name} 真实=${s.ws} 预测=${p.ws} ${(f.confidence * 100).toFixed(0)}%`)
    return
  }

  if (s.dt) {
    dtTotal++
    if (p.dt === s.dt) dtHit++
    else misses.push(`[类型偏差] ${f.name} 真实=${s.ws}/${s.dt} 预测=${p.ws}/${p.dt}`)
  }
})

const clearCount = ALL_SAMPLES.filter((s) => s.ws !== null).length
const wsAcc = wsTotal ? wsHit / wsTotal : 0
const dtAcc = dtTotal ? dtHit / dtTotal : 0
const hardRecall = hardTotal ? hardHit / hardTotal : 1

const high = files.filter((f) => f.confidence >= 0.85).length
const mid = files.filter((f) => f.confidence >= 0.7 && f.confidence < 0.85).length
const low = files.filter((f) => f.confidence < 0.7).length

console.log('\n========== 分类引擎离线校验 ==========')
console.log(`样本总数          : ${files.length}（清晰 ${clearCount} · 难例 ${hardTotal}）`)
console.log(`归纳出一级工作域  : ${result.autoK} 个`)
for (const c of result.categories.filter((x) => !x.parentId)) {
  console.log(`    ${c.name}`)
}
console.log('')
console.log(`自动通过          : ${autoPass} 个（占清晰样本 ${((autoPass / clearCount) * 100).toFixed(0)}%）`)
console.log(`工作域准确率      : ${(wsAcc * 100).toFixed(1)}%  (${wsHit}/${wsTotal})   ← 归档正确性关键`)
console.log(`文档类型准确率    : ${(dtAcc * 100).toFixed(1)}%  (${dtHit}/${dtTotal})   ← 错了不致命`)
console.log(`难例正确分流率    : ${(hardRecall * 100).toFixed(1)}%  (${hardHit}/${hardTotal})   ← 拿不准的进待确认`)
console.log(`置信度分布        : 高(>=0.85) ${high} · 中(0.7-0.85) ${mid} · 低(<0.7) ${low}`)
console.log('')

console.log('---------- 每工作域准确率 ----------')
for (const [t, v] of [...perWs.entries()].sort((a, b) => b[1].total - a[1].total)) {
  console.log(`  ${t.padEnd(7, '　')} ${v.hit}/${v.total}  ${((v.hit / v.total) * 100).toFixed(1)}%`)
}

if (misses.length) {
  console.log('\n---------- 问题明细 ----------')
  misses.forEach((m) => console.log('  ' + m))
}

console.log('\n---------- 分类树 ----------')
for (const c of result.categories.filter((x) => !x.parentId)) {
  const subs = result.categories.filter((x) => x.parentId === c.id)
  const cnt = files.filter(
    (f) => f.categoryId === c.id || subs.some((s) => s.id === f.categoryId)
  ).length
  console.log(`  ${c.name} (${cnt})`)
  for (const s of subs) {
    const sc = files.filter((f) => f.categoryId === s.id).length
    console.log(`    └ ${s.name} (${sc})`)
  }
  const pend = files.filter(
    (f) => f.needReview && (f.categoryId === c.id || subs.some((s) => s.id === f.categoryId))
  ).length
  if (pend) console.log(`    （其中待确认 ${pend}）`)
}

console.log('\n---------- 待确认队列（前 10）----------')
files
  .filter((f) => f.needReview)
  .slice(0, 10)
  .forEach((f) => {
    const p = pathOf(f)
    console.log(
      `  ${f.name.padEnd(30).slice(0, 30)} ${(f.confidence * 100).toFixed(0).padStart(3)}%   猜测:${p.ws ?? '无'}${p.dt ? '/' + p.dt : ''}`
    )
  })

console.log('\n---------- 命名建议样例 ----------')
files
  .filter((f) => !f.needReview)
  .slice(0, 6)
  .forEach((f) => {
    console.log(`  ${f.name.padEnd(34).slice(0, 34)} → ${f.suggestedName}`)
  })

// 命名重名率
const nameCount = new Map<string, number>()
files.forEach((f) => nameCount.set(f.suggestedName, (nameCount.get(f.suggestedName) ?? 0) + 1))
const dup = Array.from(nameCount.entries()).filter(([, c]) => c > 1)
console.log(
  `\n命名重名组数: ${dup.length}（占 ${((dup.reduce((a, [, c]) => a + c, 0) / files.length) * 100).toFixed(0)}%）`
)
dup.slice(0, 3).forEach(([n, c]) => console.log(`  ×${c}  ${n}`))
console.log('')
