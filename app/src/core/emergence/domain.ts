/**
 * 分域：归档目录的第一层从哪来。
 *
 * 涌现式分类能给出「事项」（人工智能推进会、技术谱系…），但 45 个事项平铺在
 * 归档根目录下就是一堵墙。人整理文件是先分方向的——这台电脑主要在管哪些域的文件，
 * 再在每个域里放具体事项。
 *
 * 这一层**只能交给模型**：域的名字是「这个人用电脑管什么」的归纳，
 * 靠字符串统计推不出来（事项名里的公共词只能给出「智能工厂」这种同名前缀，
 * 推不出「项目管理 vs 行政事务」这种视角）。所以：
 *
 *   · 提示词里给事项名 + 份数，要求 2–6 个大类，并把每个事项归进去
 *   · 只接受声明过的大类名，模型自创的一律丢（不许它凭空造目录）
 *   · 模型不可用 / 答得不成样子 → domainsFromSharedToken 按公共开头兜一下，
 *     界面上说明「这是粗略分组」，用户可以在树里改
 *
 * 两种结果都只是**建议**：确认页的左树允许改名、拖动改域、解散域。
 */
import { BARE_ORG_WORDS, isDocKindPhrase } from './lexicon'
import { normKey } from './whoJudge'

export interface DomainTopic {
  name: string
  weight: number
}

export interface DomainResult {
  /** 大类名，按模型给出的顺序 */
  domains: string[]
  /** 事项名 → 大类名 */
  assign: Map<string, string>
}

/** 大类数量的边界。少于 2 等于没分，多于 6 就是又一层墙 */
export const MIN_DOMAINS = 2
export const MAX_DOMAINS = 6

export function buildDomainPrompt(topics: DomainTopic[]): string {
  const lines = topics.map((t, i) => `${i + 1}. ${t.name} · ${t.weight}份`)
  return `下面是从一台电脑的文档里归纳出的「事项」清单，每行是「事项 · 份数」。

请先判断：**这个人主要用这台电脑管理哪些方向的文件？**
给出 ${MIN_DOMAINS}–${MAX_DOMAINS} 个大类，每个名字 2–6 个字，它将作为归档目录的第一层。

要求：
- 大类要能概括一批事项。「项目交付」「技术研发」「行政事务」「人事培训」「财务报销」只是示意，具体用哪几个由这批文件决定，不要照抄
- 一个事项只能进一个大类
- 只使用下面清单里的事项名，不许编造
- 清单里每个事项都要出现一次

只返回 JSON：{"domains":["大类1","大类2"],"map":[{"topic":"事项","domain":"大类1"}]}

事项清单：
${lines.join('\n')}`
}

/** 模型爱在英文/数字和汉字之间插空格，比对前抹掉 */
function norm(s: string): string {
  return normKey(String(s ?? '').trim())
}

/**
 * 解析模型输出。
 * 只保留「清单里真有的」事项名和「声明过的」大类名——模型编的词一律丢弃，
 * 不给下游制造幻觉目录。（它很爱顺手把大类写成「部门/公司/组织名」这种空话。）
 */
export function parseDomainResult(raw: string, names: string[]): DomainResult {
  const empty: DomainResult = { domains: [], assign: new Map() }
  const m = raw.match(/\{[\s\S]*\}/)
  if (!m) return empty
  let obj: { domains?: unknown; map?: unknown }
  try {
    obj = JSON.parse(m[0])
  } catch {
    return empty
  }

  const declared: string[] = []
  const byNorm = new Map<string, string>()
  for (const d of Array.isArray(obj.domains) ? obj.domains : []) {
    const name = String(d ?? '').trim()
    const k = norm(name)
    if (!k || byNorm.has(k)) continue
    byNorm.set(k, name)
    declared.push(name)
  }

  const pool = new Map(names.map((n) => [norm(n), n]))
  const assign = new Map<string, string>()
  const raw_maps = obj.map
  const entries: Array<[unknown, unknown]> = []
  if (Array.isArray(raw_maps)) {
    for (const it of raw_maps) {
      if (it && typeof it === 'object') {
        const o = it as { topic?: unknown; domain?: unknown }
        entries.push([o.topic, o.domain])
      }
    }
  } else if (raw_maps && typeof raw_maps === 'object') {
    for (const [k, v] of Object.entries(raw_maps as Record<string, unknown>)) entries.push([k, v])
  }

  for (const [topic, domain] of entries) {
    const real = pool.get(norm(String(topic ?? '')))
    const dm = byNorm.get(norm(String(domain ?? '')))
    if (!real || !dm) continue
    assign.set(real, dm)
  }

  return { domains: declared, assign }
}

/** 至少两个域、覆盖大半事项、没有哪个域吃掉一切 —— 否则等于没分 */
export function isUsable(r: DomainResult, names: string[]): boolean {
  if (r.domains.length < MIN_DOMAINS) return false
  if (names.length === 0) return false
  if (r.assign.size < names.length * 0.6) return false
  const per = new Map<string, number>()
  for (const d of r.assign.values()) per.set(d, (per.get(d) ?? 0) + 1)
  const max = Math.max(...per.values())
  return max <= r.assign.size * 0.85
}

/** 兜底分组要求至少 3 个成员：2 个凑一个目录，只会多一层点击 */
const MIN_MEMBERS = 3
/** 公共开头最长取 6 字，再长就不是「域」而是事项本身了 */
const MAX_HEAD = 6

function lcp(a: string, b: string): string {
  let i = 0
  while (i < a.length && i < b.length && a[i] === b[i]) i++
  return a.slice(0, i)
}

/**
 * 规则兜底：事项名有公共开头的归成一类（智能工厂项目/技术室/产品模块…）。
 *
 * 这是**退而求其次**的结果——它只认字面，给不出「管理视角」。
 * 所以宁可保守：凑不够 3 个同类就不分，让用户在一级看到零散事项，
 * 也不要造一堆只有一个孩子的假目录。
 */
export function domainsFromSharedToken(topics: DomainTopic[]): DomainResult {
  const names = topics.map((t) => t.name)
  const cands = new Set<string>()
  for (const n of names) {
    for (let len = 2; len <= Math.min(MAX_HEAD, n.length); len++) cands.add(n.slice(0, len))
  }

  const covered = (head: string) => names.filter((n) => n.startsWith(head)).length
  const valid = [...cands].filter(
    (head) =>
      covered(head) >= MIN_MEMBERS && !BARE_ORG_WORDS.has(head) && !isDocKindPhrase(head)
  )

  // 只认最长的那个公共开头：「智能」「智能工」都是「智能工厂」的碎前缀，
  // 拿它们当目录名，用户会在「智能」底下找不到自己要的东西。
  const heads = valid.filter((h) => !valid.some((o) => o.length > h.length && o.startsWith(h)))

  const assign = new Map<string, string>()
  const domains: string[] = []
  for (const head of heads.sort((a, b) => covered(b) - covered(a) || b.length - a.length)) {
    const members = names.filter((n) => n !== head && n.startsWith(head))
    if (members.length === 0) continue
    domains.push(head)
    for (const n of members) assign.set(n, head)
  }
  return { domains, assign }
}

export interface TreeDomain<T> {
  name: string
  topics: T[]
  fileCount: number
}

export interface TopicTree<T> {
  /** 有域的事项，按文件数降序 */
  domains: TreeDomain<T>[]
  /** 没域（或域里只剩自己）的事项，直接放第一层 */
  loose: T[]
}

/**
 * 把带 domain 标记的事项装成树。**UI 和落盘都吃这个结构**，
 * 保证「界面上看到的目录」就是「真正建出来的目录」。
 *
 * 只有一个事项的域会被拆掉：`智能工厂\智能工厂项目\` 这种单孩子目录
 * 只是让人多点一次，而且归档根目录下看着更乱。
 */
export function buildTree<T extends { domain?: string; fileIds: string[] }>(
  topics: T[]
): TopicTree<T> {
  const byDomain = new Map<string, T[]>()
  const loose: T[] = []
  for (const t of topics) {
    const d = (t.domain ?? '').trim()
    if (!d) {
      loose.push(t)
      continue
    }
    const arr = byDomain.get(d)
    if (arr) arr.push(t)
    else byDomain.set(d, [t])
  }

  const byCount = <U extends { fileIds: string[] }>(a: U, b: U) =>
    b.fileIds.length - a.fileIds.length || 0
  const domains: TreeDomain<T>[] = []
  for (const [name, list] of byDomain) {
    if (list.length < 2) {
      for (const t of list) loose.push({ ...t, domain: undefined })
      continue
    }
    const sorted = [...list].sort(byCount)
    domains.push({ name, topics: sorted, fileCount: sorted.reduce((n, t) => n + t.fileIds.length, 0) })
  }
  domains.sort((a, b) => b.fileCount - a.fileCount || a.name.localeCompare(b.name, 'zh'))
  loose.sort(byCount)
  return { domains, loose }
}
