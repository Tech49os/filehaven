/**
 * 模型层：把算法抽出的脏候选，变成用户能看懂的事项清单
 *
 * 为什么必须由模型做这一步（原型实测的依据）：
 *  - `智能工厂技术室`(28) 和 `技术谱系`(18) 在统计特征上无法区分，
 *    一个是部门名、一个是事项，只有懂语义的模型能分辨
 *  - `智造领航`、`逐浪数字蓝海`、`AI大赛` 是同一份参赛材料的三种叫法，
 *    纯靠字符串永远合不到一起
 *
 * 关键设计：**让它做判断题，不让它做归纳题**。
 * 第一版 prompt 写的是「请合并同一件事的不同叫法」，结果 7b 模型把 80 个候选
 * 硬压成 5 个笼统大类（智能工厂推进、数字化建设、AI应用赋能……），还把
 * 钻采院、差旅费、AI大赛全丢进 noise——这正是老版本「一口吞掉」的翻版。
 * 改成逐条判定（每个词要么归到某件事，要么标 null）之后，它才肯分细。
 */
import type { Candidate } from './candidates'
import { DOC_KIND_WORDS } from './lexicon'

export interface MergedTopic {
  name: string
  desc: string
  aliases: string[]
}

export interface MergeParseResult {
  topics: MergedTopic[]
  /** 模型判为「不是一件具体的事」的词组 */
  noise: string[]
  /** 模型编造的、不在候选池里的词（用于排查模型是否在乱写） */
  rejected: string[]
}

export function buildMergePrompt(cands: Candidate[]): string {
  const lines = cands.map((c, i) => {
    const org = c.isOrg ? '（疑似部门/公司名）' : ''
    return `${i + 1}. ${c.term}${org} · ${c.df} 份 · 例：${c.samples.join(' / ')}`
  })
  return `下面是从一批文件的文件名里抽出的高频词组，每个词组后面是出现次数和代表文件名。

任务：把这些词组分组——**同一件事的不同叫法放一组**，并给每组起一个名字。

规则：
1. 除了明确的「部门名 / 公司名 / 人名 / 纯文档格式词」，每个词组都要归到某个事项里。
   部门名如「XX技术室」「XX项目组」，公司名如「XX公司」「XX集团」，
   纯文档格式词如「演讲稿」「申请表」「操作手册」。
2. **没把握就让它单独成组，不要标 null 图省事**。一个词一组完全可以接受。
3. 同一件事的不同叫法必须用同一个组名（比如同一份材料的不同版本名、全称和简称）。
4. 组名不超过 10 个字，要具体。「XX项目」「XX工作」这种放之四海皆准的名字没有用。

示例（只示意分组方式，不是候选）：
  「A厂设备改造」「A厂改造方案」→ 同组，组名「A厂设备改造」
  「年度培训计划」→ 单独一组，组名「年度培训计划」
  「XX技术室」→ 部门名，标 null

候选词组：
${lines.join('\n')}

只返回 JSON，每个词组一条，不要解释：
{"items":[{"term":"词组原文","topic":"组名或 null"}]}`
}

export function parseMergeResult(raw: string, cands: Candidate[]): MergeParseResult {
  const m = raw.match(/\{[\s\S]*\}/)
  if (!m) throw new Error('模型没有返回 JSON')
  let obj: { items?: unknown }
  try {
    obj = JSON.parse(m[0])
  } catch {
    throw new Error('模型返回的 JSON 无法解析')
  }
  const pool = new Set(cands.map((c) => c.term))
  const rejected: string[] = []
  const noise: string[] = []
  /** topic 名 → aliases */
  const groups = new Map<string, string[]>()
  const order: string[] = []

  for (const item of Array.isArray(obj.items) ? obj.items : []) {
    if (!item || typeof item !== 'object') continue
    const { term, topic } = item as { term?: unknown; topic?: unknown }
    const t = String(term ?? '').trim()
    if (!t) continue
    if (!pool.has(t)) {
      rejected.push(t)
      continue
    }
    if (topic === null || topic === undefined || topic === '' || topic === 'null') {
      noise.push(t)
      continue
    }
    const name = String(topic).trim().slice(0, 12)
    if (!name) {
      noise.push(t)
      continue
    }
    const arr = groups.get(name)
    if (arr) {
      if (!arr.includes(t)) arr.push(t)
    } else {
      groups.set(name, [t])
      order.push(name)
    }
  }

  const topics: MergedTopic[] = order.map((name) => ({
    name,
    desc: '',
    aliases: groups.get(name)!,
  }))
  if (topics.length === 0) throw new Error('模型没有给出任何有效分类')
  return { topics, noise, rejected }
}

/**
 * 降级方案：模型不可用时也能给出可用的初步分类。
 *
 * 直接用非组织候选当类名——名字可能不够漂亮（比如「梯度培育培训课程设置」
 * 而不是「梯度培育」），但界面会提示「未启用模型，建议改名」，用户改一下即可。
 * 宁可给一个粗糙但能用的方案，也不要让整个工具因为没装模型而不可用。
 */
export function fallbackMerge(cands: Candidate[], k = 12): MergedTopic[] {
  return cands
    .filter((c) => !c.isOrg && !DOC_KIND_WORDS.has(c.term))
    .slice()
    .sort((a, b) => b.score - a.score)
    .slice(0, k)
    .map((c) => ({ name: c.term, desc: '', aliases: [c.term] }))
}
