/**
 * 模型 spike v3：只问一件事 —— 这个词是人名/组织名吗？
 *
 * v2 的结论（两个模型、同一批 57 个候选、各 30-38s）：
 *   · 组织名/人名 判得很准：海油发展、海油发展数字化、集团、信科公司、
 *     中海油、中国海油、生产智能化项目组、智能工厂技术室、王一名 全中
 *   · 但「空话 vs 真事项」判坏了：9b 把技术谱系/境内差旅费/安环大屏/技术沉淀
 *     全塞进 generic，4b 全塞进 doc —— 这类边界它俩都没有判断力，
 *     而字符串规则在这件事上本来就更强
 *
 * 所以只保留模型真正擅长的这一问：是不是人名/组织名。
 * 命中 → 不进类目（它说不出「哪件事」）；其余全部交给现有规则。
 *
 * 用法：
 *   npx tsx test/model-org-spike.ts build
 *   npx tsx test/model-org-spike.ts score merge-org-9b.json 9b
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates, type Candidate } from '../src/core/emergence/candidates'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'

const here = dirname(fileURLToPath(import.meta.url))

/** 人工认定的人名/组织名（这份语料里的「谁」，不是「哪件事」） */
const IS_WHO = [
  '海油发展', '海油发展数字化', '海油发展智能工厂', '集团', '中海油', '中国海油',
  '信科公司', '智能工厂技术室', '生产智能化项目组', '王一名',
]

/** 人工认定绝对不是人名/组织名的（含容易被带偏的：带地名的、带角色的） */
const NOT_WHO = [
  '技术谱系', '人工智能推进会', '梯度培育培训课程设置', '智擎海发版', 'AI编程',
  'AI大赛', '中期工作会', '全栈工程师', '安环大屏', '境内差旅费',
  '智能工厂产品模块', '模块深化建设项目', '非功能性需求管控', '技术沉淀',
  '作业H5培训游戏', '数字化建设', '智驱生产', '海上采油平台受限',
  '赋能海发工业互联网平台', '智造领航逐浪数字蓝海', '服务器领用交接单',
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

export function buildOrgPrompt(cands: Candidate[]): string {
  const lines = cands.map((c, i) => {
    const sample = (c.samples[0] ?? '').slice(0, 24)
    return `${i + 1}. ${c.term} · ${c.df}份 · ${sample}`
  })
  return `下面是从一批文件名里抽出的高频词组，每行是「词组 · 出现份数 · 一个代表文件名」。

只回答一个问题：**这个词组是人名或组织名吗？**

- person：某个人的姓名
- org：部门、公司、单位、团队、项目组的名称（"谁在办"）
- no：都不是（"哪件事"、"什么东西"）

注意：带地名的项目名不是组织名（「A厂设备改造」是事，「A厂技术室」才是组织）；
职位/角色不是人名（「全栈工程师」不指某个具体的人）。

候选词组：
${lines.join('\n')}

每个词组都要出现一次，不许漏，不许编造。
只返回 JSON：{"items":[{"term":"词组","kind":"org"}]}`
}

/** 模型爱在英文/数字和汉字之间插空格（AI 编程 / 作业 H5 培训游戏），比对前先抹掉 */
export function normKey(s: string): string {
  return s.replace(/\s+/g, '')
}

const mode = process.argv[2] ?? 'build'

if (mode === 'build') {
  const cands = loadCandidateTerms()
  const prompt = buildOrgPrompt(cands)
  writeFileSync(resolve(here, 'merge-org-prompt.txt'), prompt, 'utf-8')
  console.log(`候选 ${cands.length} 个 · prompt ${prompt.length} 字`)
} else if (mode === 'score') {
  const file = process.argv[3] ?? 'merge-org-9b.json'
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
  const pool = new Map(cands.map((c) => [normKey(c.term), c.term]))
  const kindOf = new Map<string, string>()
  let hallucinated = 0
  for (const it of obj.items ?? []) {
    const raw = String(it.term ?? '').trim()
    const real = pool.get(normKey(raw))
    const k = String(it.kind ?? '').trim().toLowerCase()
    if (!real) {
      hallucinated++
      continue
    }
    if (!['org', 'person', 'no'].includes(k)) continue
    kindOf.set(real, k)
  }
  const missing = cands.map((c) => c.term).filter((t) => !kindOf.has(t))

  const hitWho = IS_WHO.filter((t) => kindOf.get(t) === 'org' || kindOf.get(t) === 'person')
  const missWho = IS_WHO.filter((t) => !hitWho.includes(t))
  const wrongWho = NOT_WHO.filter((t) => kindOf.get(t) === 'org' || kindOf.get(t) === 'person')

  // 叠加前缀扩散后（产品里就是这么用的）
  const { expandWho } = await import('../src/core/emergence/whoJudge')
  const baseMap = new Map<string, 'org' | 'person' | 'no'>()
  for (const [t, k] of kindOf) baseMap.set(t, k as 'org' | 'person' | 'no')
  const expanded = expandWho(baseMap, cands)
  const hitWho2 = IS_WHO.filter((t) => expanded.has(t))
  const wrongWho2 = NOT_WHO.filter((t) => expanded.has(t))

  console.log(`\n=== ${label} ===`)
  console.log(`回填 ${kindOf.size}/${cands.length} · 编造 ${hallucinated} · 漏答 ${missing.length}`)
  if (missing.length) console.log(`  漏：${missing.join('、')}`)
  console.log(`组织/人名 抓到 ${hitWho.length}/${IS_WHO.length}${missWho.length ? `（漏 ${missWho.join('、')}）` : ' 全中'}`)
  console.log(`误伤 ${wrongWho.length}/${NOT_WHO.length}${wrongWho.length ? `：${wrongWho.join('、')}` : ' 无误伤'}`)
  const precision = hitWho.length / Math.max(1, hitWho.length + wrongWho.length)
  console.log(
    `准确率 ${(precision * 100).toFixed(0)}% · 召回 ${((hitWho.length / IS_WHO.length) * 100).toFixed(0)}%`
  )
  console.log(
    `+前缀扩散：抓到 ${hitWho2.length}/${IS_WHO.length} · 误伤 ${wrongWho2.length}/${NOT_WHO.length}` +
      `${wrongWho2.length ? `：${wrongWho2.join('、')}` : ''}`
  )
  const expandedOut = [...expanded]
  writeFileSync(
    resolve(here, `merge-org-score-${label}.json`),
    JSON.stringify(
      { label, kindOf: Object.fromEntries(kindOf), missing, hitWho, missWho, wrongWho, expanded: expandedOut },
      null,
      1
    ),
    'utf-8'
  )
} else {
  console.log('用法：model-org-spike.ts build | score <rawFile> [label]')
}
