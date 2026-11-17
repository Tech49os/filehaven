/**
 * 提案装配：把「涌现出的候选分组」变成「每个文件归到哪个事项」的可确认方案
 *
 * 关键点：这一步不写文件、不建目录，只产出内存里的提案。
 * 用户确认方案之后才真正归类——这是「先提案后执行」的分界线。
 */
import type { Category, FileItem } from '../types'
import type { TopicGroup } from './grouping'

export interface ProposalTopic {
  id: string
  name: string
  aliases: string[]
  /** 归入本事项的文件 id */
  fileIds: string[]
  /** 代表文件名（确认页展示，最多 5 个） */
  samples: string[]
}

export interface Proposal {
  topics: ProposalTopic[]
  /** 没能归到任何事项的文件 id —— 必须让用户看到，绝不藏起来 */
  ungroupedIds: string[]
}

const MAX_SAMPLES = 5

/** 文件名命中权重高于正文：作者命名时一定会带事项名 */
const NAME_HIT = 0.9
const BODY_HIT = 0.75

function countOccur(hay: string, needle: string): number {
  if (!needle) return 0
  let n = 0
  let i = 0
  while ((i = hay.indexOf(needle, i)) !== -1) {
    n++
    i += needle.length
  }
  return n
}

/**
 * 命中打分。
 *
 * 正文判定比文件名严格得多，这是真实数据逼出来的：
 * 一份「报价单」的正文里提到一次「软件开发」，若按「提到就算」，
 * 它会被归到「软件开发」事项下——而它其实是商务采购的报价单。
 * 所以正文必须**重复提到 2 次**，或者词本身够长（≥5 字，专指性足够强）才算数。
 */
function hitScore(f: FileItem, alias: string): number {
  const a = alias.toLowerCase()
  if (!a) return 0
  if (f.name.toLowerCase().includes(a)) return NAME_HIT
  const body = `${f.textSummary ?? ''} ${(f.drawioNodes ?? []).join(' ')}`.toLowerCase()
  const n = countOccur(body, a)
  if (n === 0) return 0
  return n >= 2 || a.length >= 5 ? BODY_HIT : 0
}

/**
 * 给一份文件找最合适的事项。
 *
 * 排序依据（依次）：
 *  1. 证据强度：文件名命中 > 只看正文
 *  2. 别名长度：「人工智能推进会」比「人工智能」具体
 *  3. **分类的覆盖规模**：范围小的优先
 *
 * 第 3 条是真实数据逼出来的：`海油发展`（公司名）和 `技术谱系`（真事项）同为
 * 4 个字、都在文件名里，若不看覆盖规模，一份「海油发展技术谱系V1.3」会被
 * 前 33 份的大类抢走，导致真正的技术谱系文档散掉。
 */
export function matchTopic(
  f: FileItem,
  topics: ProposalTopic[],
  scale?: Map<string, number>
): ProposalTopic | null {
  let best: { topic: ProposalTopic; score: number; len: number; scale: number } | null = null
  for (const t of topics) {
    for (const alias of t.aliases) {
      const s = hitScore(f, alias)
      if (s === 0) continue
      const len = alias.length
      const sc = scale?.get(t.id) ?? 0
      const better =
        !best ||
        s > best.score ||
        (s === best.score && len > best.len) ||
        (s === best.score && len === best.len && sc < best.scale)
      if (better) best = { topic: t, score: s, len, scale: sc }
    }
  }
  return best?.topic ?? null
}

function fill(files: FileItem[], topics: ProposalTopic[], keepEmpty = false): Proposal {
  const byId = new Map(topics.map((t) => [t.id, t]))
  const fresh: ProposalTopic[] = topics.map((t) => ({ ...t, fileIds: [], samples: [] }))
  const freshById = new Map(fresh.map((t) => [t.id, t]))
  const ungroupedIds: string[] = []

  // 每个事项能覆盖多少文件 —— 用来判断谁更「专指」
  const scale = new Map<string, number>()
  for (const t of fresh) {
    let n = 0
    for (const f of files) {
      if (t.aliases.some((a) => hitScore(f, a) > 0)) {
        n++
        if (n > files.length) break
      }
    }
    scale.set(t.id, n)
  }

  for (const f of files) {
    const t = matchTopic(f, fresh, scale)
    if (!t) {
      ungroupedIds.push(f.id)
      continue
    }
    const target = freshById.get(t.id) ?? byId.get(t.id)!
    target.fileIds.push(f.id)
    if (target.samples.length < MAX_SAMPLES) target.samples.push(f.name)
  }

  return {
    topics: keepEmpty ? fresh : fresh.filter((t) => t.fileIds.length > 0),
    ungroupedIds,
  }
}

export function buildProposal(files: FileItem[], groups: TopicGroup[]): Proposal {
  const topics: ProposalTopic[] = groups.map((g, i) => ({
    id: `t${i + 1}`,
    name: g.name,
    aliases: [...g.aliases],
    fileIds: [],
    samples: [],
  }))
  return fill(files, topics)
}

/**
 * 用户改名 / 删除 / 合并之后重算归属。
 * 纯 CPU，毫秒级——所以界面可以「改一下就立刻重算」，不用重新扫描或调模型。
 */
export function reassign(files: FileItem[], topics: ProposalTopic[]): Proposal {
  // 编辑场景保留空事项：用户刚删完文件时那个类还在界面上，不然会"自己消失"
  return fill(files, topics, true)
}

/** 把确认过的提案落成「分类目录 + 文件归属」，迁移页和表格都吃这个结构 */
export interface AppliedProposal {
  categories: Category[]
  files: FileItem[]
}

export function applyProposal(files: FileItem[], p: Proposal): AppliedProposal {
  const idByName = new Map<string, string>()
  const categories: Category[] = p.topics.map((t, i) => {
    const id = `ws_${i}`
    idByName.set(t.name, id)
    return {
      id,
      name: t.name,
      desc: `${t.fileIds.length} 份文件`,
      parentId: null,
      order: i,
      isFromAI: true,
    }
  })

  const topicOfFile = new Map<string, string>()
  for (const t of p.topics) for (const fid of t.fileIds) topicOfFile.set(fid, t.name)

  const out = files.map((f) => {
    const topic = topicOfFile.get(f.id) ?? null
    const year = String(new Date(f.mtime).getFullYear())
    const tags = topic ? [topic, year] : [year]
    if (f.kind === 'drawio') tags.push('架构图')
    return {
      ...f,
      ws: topic,
      dt: null,
      categoryId: topic ? (idByName.get(topic) ?? null) : null,
      confidence: topic ? 0.9 : 0.2,
      needReview: !topic,
      status: topic ? ('classified' as const) : ('pending' as const),
      reason: topic ? `归入事项「${topic}」` : '没有匹配到任何事项，请手动指定',
      aiSource: topic ? ('rule' as const) : ('none' as const),
      tags,
    }
  })

  return { categories, files: out }
}
