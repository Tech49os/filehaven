/**
 * 模型归并 spike（把「造 prompt」和「调模型」拆开）
 *
 * 为什么拆开：上一版在脚本里直接 fetch，9b 在 num_ctx 8192 下被显存挤到 CPU
 * 上跑，整整 59 分钟没有任何输出，AbortSignal 也没把它拉回来。
 * 现在传输交给 curl（--max-time 硬超时、行为可观测），脚本只负责造 prompt 和打分。
 *
 * 用法：
 *   npx tsx test/model-merge-spike.ts build              # 生成 test/merge-prompt.txt
 *   npx tsx test/model-merge-spike.ts score merge-9b.json # 解析并打分
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates, type Candidate } from '../src/core/emergence/candidates'
import { parseMergeResult } from '../src/core/emergence/merge'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'

const here = dirname(fileURLToPath(import.meta.url))

/** 人工认定的真事项（这份语料里确实在办的事） */
const GOOD = [
  '技术谱系', '人工智能推进会', '梯度培育培训课程设置', '智擎海发版', 'AI编程',
  'AI大赛', '中期工作会', '全栈工程师', '安环大屏', '境内差旅费',
  '智能工厂产品模块', '模块深化建设项目', '非功能性需求管控', '技术沉淀',
  '作业H5培训游戏', '数字化建设',
]

/** 人工认定的垃圾词（组织名 / 人名 / 文种 / 空话） */
const JUNK = [
  '集团', '软件开发', '生产管理', '架构设计', '王一名', '中海油', '中国海油',
  '演讲稿', '进展汇报', '操作手册', 'ppt修改', '资源申请单',
  '化项目建设', '发展智能工厂系统', '培训项目项目方案', '服务器配置',
]

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

任务：把每个词组归类。
- 同一件事的不同叫法 → 填同一个组名
- 不是一件具体的事 → 组名填 null

判 null 的只有四类：部门/公司/组织名、人名、只说文件是什么的词（演讲稿/操作手册/进展汇报）、
放之四海皆准的空话（生产管理/软件开发/架构设计）。

示例（不是候选）：
「A厂设备改造」「A厂改造方案」→ 两个都填「A厂设备改造」
「A厂技术室」→ null
「年度培训计划」→ 填「年度培训计划」（一个词单独成组完全可以）

硬要求：
1. 每个词组必须出现一次，不许漏，不许编造候选里没有的词
2. 没把握就让那个词单独成组、组名就用它自己——不要为了少输出而填 null
3. 组名不超过 10 个字，要具体

候选词组：
${lines.join('\n')}

只返回 JSON，不要解释：
{"items":[{"term":"词组","topic":"组名或null"}]}`
}

const mode = process.argv[2] ?? 'build'

if (mode === 'build') {
  const cands = loadCandidateTerms()
  const prompt = buildPrompt(cands)
  const out = resolve(here, 'merge-prompt.txt')
  writeFileSync(out, prompt, 'utf-8')
  console.log(`候选 ${cands.length} 个 · prompt ${prompt.length} 字（约 ${Math.round(prompt.length / 1.6)} token）`)
  console.log(`已写入 ${out}`)
} else if (mode === 'score') {
  const file = process.argv[3] ?? 'merge-raw.json'
  const label = process.argv[4] ?? file
  const cands = loadCandidateTerms()
  let text = readFileSync(resolve(here, file), 'utf8')
  // curl 存的是 /api/chat 的完整响应，取出 content
  if (text.trimStart().startsWith('{') && text.includes('"message"')) {
    try {
      const j = JSON.parse(text) as { message?: { content?: string } }
      if (j.message?.content) text = j.message.content
    } catch {
      /* 本身就是模型裸输出 */
    }
  }
  const p = parseMergeResult(text, cands)
  const topicOf = new Map<string, string>()
  for (const t of p.topics) for (const a of t.aliases) topicOf.set(a, t.name)

  const keptGood = GOOD.filter((g) => topicOf.has(g))
  const killedJunk = JUNK.filter((j) => p.noise.includes(j))
  console.log(`\n=== ${label} ===`)
  console.log(`事项 ${p.topics.length} 个 · 判为空话 ${p.noise.length} 个 · 编造 ${p.rejected.length} 个`)
  console.log(`真事项保住 ${keptGood.length}/${GOOD.length}`)
  console.log(`  漏掉：${GOOD.filter((g) => !keptGood.includes(g)).join('、') || '（无）'}`)
  console.log(`垃圾清掉 ${killedJunk.length}/${JUNK.length}`)
  console.log(`  漏网：${JUNK.filter((j) => !killedJunk.includes(j)).join('、') || '（无）'}`)
  console.log('\n=== 事项清单 ===')
  p.topics
    .slice()
    .sort((a, b) => b.aliases.length - a.aliases.length)
    .forEach((t, i) => {
      console.log(`${String(i + 1).padStart(2)}. ${t.name.padEnd(14, ' ')} ${t.aliases.join('、')}`)
    })
  writeFileSync(
    resolve(here, `merge-score-${label}.json`),
    JSON.stringify({ label, ...p, keptGood, killedJunk }, null, 1),
    'utf-8'
  )
} else {
  console.log('用法：model-merge-spike.ts build | score <rawFile> [label]')
}
