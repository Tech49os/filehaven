/**
 * 模型 spike v2：只让模型做「这个词是哪一类」的判断题
 *
 * 为什么改：第一版让模型「归并并命名」，4b 把组名直接写成「部门/公司/组织名」
 * 「放之四海皆准的空话」——它其实把我的四类判据当成了组名，说明它在做「分类」
 * 而不是「归纳」。但它的分桶结果很准：海油发展/集团/信科公司/中海油/王一名
 * 全进了组织或人名，这些正是字符串规则杀不掉的。
 *
 * 所以分工改成：
 *   模型只回答 kind ∈ {matter, org, person, doc, generic}（封闭选项，不会自由发挥）
 *   合并别名仍然交给字符串规则（包含关系），类名取最专指的那个
 *
 * 用法：
 *   npx tsx test/model-kind-spike.ts build
 *   npx tsx test/model-kind-spike.ts score merge-kind-4b.json 4b
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates, type Candidate } from '../src/core/emergence/candidates'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'

const here = dirname(fileURLToPath(import.meta.url))

/** 人工认定的真事项 */
const GOOD = [
  '技术谱系', '人工智能推进会', '梯度培育培训课程设置', '智擎海发版', 'AI编程',
  'AI大赛', '中期工作会', '全栈工程师', '安环大屏', '境内差旅费',
  '智能工厂产品模块', '模块深化建设项目', '非功能性需求管控', '技术沉淀',
  '作业H5培训游戏', '数字化建设',
]

/** 人工认定的垃圾词 */
const JUNK = [
  '集团', '软件开发', '生产管理', '架构设计', '王一名', '中海油', '中国海油',
  '演讲稿', '进展汇报', '操作手册', 'ppt修改', '资源申请单',
  '化项目建设', '发展智能工厂系统', '培训项目项目方案', '服务器配置',
]

export const KINDS = ['matter', 'org', 'person', 'doc', 'generic'] as const
export type Kind = (typeof KINDS)[number]

function loadCandidateTerms(): Candidate[] {
  const raw = JSON.parse(readFileSync(resolve(here, 'desktop-scan.json'), 'utf8')) as {
    files: { name: string; path: string }[]
  }
  const files = raw.files.map((f) => ({ name: f.name, path: f.path }))
  const cands = collectCandidates(files, { topK: 60 })
  const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
  return cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))
}

function buildPrompt(cands: Candidate[]): string {
  const lines = cands.map((c, i) => {
    const sample = (c.samples[0] ?? '').slice(0, 24)
    return `${i + 1}. ${c.term} · ${c.df}份 · ${sample}`
  })
  return `下面是从 455 份文件的文件名里抽出的高频词组。每行是「词组 · 出现份数 · 一个代表文件名」。

给每个词组判一个 kind，**只能从这五个值里选一个**：
- matter：一件具体在办的事（项目、专题、会议、某批交付物所属的那件事）
- org：部门 / 公司 / 组织名
- person：人名
- doc：只说了这是什么文件、没说是哪件事（演讲稿、操作手册、申请单、汇报）
- generic：放之四海皆准的泛词，说不出具体是哪件事（生产管理、软件开发、架构设计）

判断方法：把这个词当文件夹名给同事看，他能不能一眼知道里面放的是哪件事。
能 → matter。不能 → 在其余四类里挑最像的。

示例（不是候选）：
「A厂设备改造」→ matter
「A厂改造方案」→ matter（虽然是文种词，但前面挂着具体的事）
「A厂技术室」→ org
「张某某」→ person
「演讲稿」→ doc
「年度工作」→ generic

硬要求：
1. 每个词组必须出现一次，不许漏，不许编造候选里没有的词
2. kind 只能填 matter / org / person / doc / generic 这五个英文词之一

候选词组：
${lines.join('\n')}

只返回 JSON：
{"items":[{"term":"词组","kind":"matter"}]}`
}

const mode = process.argv[2] ?? 'build'

if (mode === 'build') {
  const cands = loadCandidateTerms()
  const prompt = buildPrompt(cands)
  writeFileSync(resolve(here, 'merge-kind-prompt.txt'), prompt, 'utf-8')
  console.log(`候选 ${cands.length} 个 · prompt ${prompt.length} 字（约 ${Math.round(prompt.length / 1.6)} token）`)
} else if (mode === 'score') {
  const file = process.argv[3] ?? 'merge-kind-4b.json'
  const label = process.argv[4] ?? file
  const cands = loadCandidateTerms()
  let text = readFileSync(resolve(here, file), 'utf8')
  if (text.trimStart().startsWith('{') && text.includes('"message"')) {
    const j = JSON.parse(text) as { message?: { content?: string } }
    if (j.message?.content) text = j.message.content
  }
  const m = text.match(/\{[\s\S]*\}/)
  if (!m) throw new Error('没返回 JSON')
  const obj = JSON.parse(m[0]) as { items?: { term?: string; kind?: string }[] }
  const pool = new Set(cands.map((c) => c.term))
  const kindOf = new Map<string, string>()
  let hallucinated = 0
  for (const it of obj.items ?? []) {
    const t = String(it.term ?? '').trim()
    const k = String(it.kind ?? '').trim().toLowerCase()
    if (!pool.has(t)) {
      hallucinated++
      continue
    }
    if (!KINDS.includes(k as Kind)) continue
    kindOf.set(t, k)
  }
  const missing = cands.map((c) => c.term).filter((t) => !kindOf.has(t))

  const keptGood = GOOD.filter((g) => kindOf.get(g) === 'matter')
  const killedJunk = JUNK.filter((j) => kindOf.has(j) && kindOf.get(j) !== 'matter')
  console.log(`\n=== ${label} ===`)
  console.log(`回填 ${kindOf.size}/${cands.length} · 编造 ${hallucinated} · 漏答 ${missing.length}`)
  if (missing.length) console.log(`  漏：${missing.join('、')}`)
  console.log(`真事项判对 ${keptGood.length}/${GOOD.length}`)
  const badGood = GOOD.filter((g) => kindOf.has(g) && kindOf.get(g) !== 'matter')
  if (badGood.length) console.log(`  判错：${badGood.map((g) => `${g}=${kindOf.get(g)}`).join('、')}`)
  console.log(`垃圾清掉 ${killedJunk.length}/${JUNK.length}`)
  const survived = JUNK.filter((j) => kindOf.get(j) === 'matter' || !kindOf.has(j))
  if (survived.length) console.log(`  漏网：${survived.map((j) => `${j}=${kindOf.get(j) ?? '未答'}`).join('、')}`)

  console.log('\n=== 被判为非事项的词 ===')
  for (const k of ['org', 'person', 'doc', 'generic']) {
    const list = [...kindOf].filter(([, v]) => v === k).map(([t]) => t)
    console.log(`${k.padEnd(8)} ${list.join('、') || '（无）'}`)
  }
  writeFileSync(
    resolve(here, `merge-kind-score-${label}.json`),
    JSON.stringify({ label, kindOf: Object.fromEntries(kindOf), missing, keptGood, killedJunk }, null, 1),
    'utf-8'
  )
} else {
  console.log('用法：model-kind-spike.ts build | score <rawFile> [label]')
}
