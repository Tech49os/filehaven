/**
 * 同一文档的多个版本（下称「版本族」）
 *
 * 真实场景：455 份文件里 137 份（30%）属于 51 个版本族，最长的一族 7 个版本
 * （同一个集团汇报 PPT 的 V1.1/V1.2/V1.3/V2.0/无版本号，每个 78–110MB）。
 * 归档时如果按原样平铺，目录里就是一堆看着一样的名字，用户根本不知道该用哪份。
 *
 * 本模块只做三件事，都不改文件内容：
 *  1. 解析文件名里的版本痕迹（V1.1 / 第3版 / 终版 / (1) / 副本）
 *  2. 把「同一份文档的不同版本」归族，族内按修改时间排，最新的在最前
 *  3. 给出规范化后的文件名：版本标记统一挪到末尾括号（方案V1.docx → 方案 (v1).docx），
 *     没痕迹的那份保留原名，单份文件一律不动
 *
 * 为什么不用哈希判定：同一文档的不同版本内容必然不同，哈希反而分不出来。
 * 哈希留给「字节完全相同的副本」那条线（目前只标注、不删）。
 *
 * 两条踩过的坑（都有真实文件兜着，别改回去）：
 *  · 日期不能当版本痕迹改名。`人工智能推进会-生产智能化项目组-2026.8.4.pptx` 里的
 *    日期是会议日期，改名成 `… (2026-08-04)` 就变成「这文件有多个日期版本」的意思了。
 *    日期只用于判断「是不是同一份」（家族键里去掉），展示和改名都保留原位。
 *  · 同主名不同扩展不能一律并族。`work-item-detail.html/.js/.css` 是三个不同文件，
 *    不是一份文档的三种格式。只有同扩展、或同为文档/表格/幻灯/PDF 时才并。
 */

export type MarkKind = 'ver' | 'copy' | 'date'

export interface Mark {
  kind: MarkKind
  /** 展示用的规范标签：v1.1 / 副本2 / 2024-03-15 / 终版 */
  label: string
  /** 排序用，越大越新；同族内实际排序仍以修改时间为准 */
  order: number
}

export interface Parsed {
  /** 归一化主名：只用来判「是不是同一份文档」，不展示（日期也去掉） */
  key: string
  /** 去掉版本/副本痕迹后的显示名；日期留在原位——它常常是名字的一部分 */
  title: string
  /** 只含 ver / copy；date 不算「版本痕迹」，不参与改名 */
  marks: Mark[]
}

export interface FamFile {
  id: string
  name: string
  size: number
  mtime: number
}

export interface Family<T extends FamFile = FamFile> {
  key: string
  title: string
  /** 已按「最新在前」排好 */
  files: T[]
  latestId: string
}

/** 「只差一个修饰词」的收尾词，按新旧给个先后，实际排序仍以 mtime 为准 */
const WORD_ORDER: Record<string, number> = {
  初版: 0,
  草稿: 10,
  修改: 500,
  修改版: 500,
  修订: 500,
  修订版: 500,
  完: 600,
  最新版: 900,
  终稿: 900,
  终版: 900,
  最终版: 900,
  定稿: 950,
}

const CN_DIGIT: Record<string, number> = {
  一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9, 十: 10,
}

function cnNum(s: string): number {
  if (/^\d+$/.test(s)) return Number(s)
  if (s === '十') return 10
  if (s.length === 1) return CN_DIGIT[s] ?? 0
  if (s.startsWith('十')) return 10 + (CN_DIGIT[s[1]] ?? 0)
  if (s.endsWith('十')) return (CN_DIGIT[s[0]] ?? 0) * 10
  const [t, o] = s.split('十')
  return (CN_DIGIT[t] ?? 0) * 10 + (CN_DIGIT[o] ?? 0)
}

const SEP = '[\\s_\\-—·]*'

/**
 * 从尾巴上啃掉一个痕迹。allow 之外的种类**不啃**（返回 null 就停手），
 * 这样算显示名时遇到日期会直接收工，不会把日期后面的版本号也吃掉。
 *
 * 顺序要紧：先认 (1) 这种带括号的，再认 V1.1，最后才认「终版」这类词。
 * 全部锚在 $ 上——只处理文件名结尾，不会把标题里的「2024年度」当日期剥掉。
 */
function takeTrailingMark(s: string, allow: Set<MarkKind>): { rest: string; mark: Mark } | null {
  let m: RegExpMatchArray | null

  if (allow.has('copy')) {
    // Windows 重复下载：(1)(2)……原件算第 1 份，所以 (1) → 副本2
    if ((m = s.match(new RegExp(`^(.*?)${SEP}[\\(（]\\s*(\\d{1,3})\\s*[\\)）]\\s*$`)))) {
      const n = Number(m[2]) + 1
      return { rest: m[1], mark: { kind: 'copy', label: `副本${n}`, order: n } }
    }
  }

  if (allow.has('ver')) {
    // V1 / v1.2 / V2.0版本
    if ((m = s.match(new RegExp(`^(.*?)${SEP}[vV]\\s*(\\d+(?:\\.\\d+)*)\\s*(?:版本)?\\s*$`)))) {
      return { rest: m[1], mark: { kind: 'ver', label: `v${m[2]}`, order: Number(m[2]) } }
    }
    // 第3版 / 第三版
    if ((m = s.match(/^(.*?)第\s*([0-9一二三四五六七八九十百]+)\s*版\s*$/))) {
      return { rest: m[1], mark: { kind: 'ver', label: `第${m[2]}版`, order: cnNum(m[2]) } }
    }
    // 「终版 / 定稿 / 初版草稿」这类词没有数字，判起来最危险：
    // `ppt修改.pptx` 里的「修改」是动词，剥掉就只剩 `ppt` 了。
    // 所以分两档——强标记（本身就是「这是最后一版」）随便剥，
    // 弱标记（修改/修订/完）必须前面有分隔符才算版本痕迹。
    const strong = /^(.*?)(初版|草稿|终版|最终版|定稿|终稿|最新版)$/
    const weak = new RegExp(`^(.*?)[\\s_\\-—·]+(修改版|修订版|修改|修订|完)$`)
    if ((m = s.match(strong))) {
      return { rest: m[1], mark: { kind: 'ver', label: m[2], order: WORD_ORDER[m[2]] ?? 500 } }
    }
    if ((m = s.match(weak))) {
      return { rest: m[1], mark: { kind: 'ver', label: m[2], order: WORD_ORDER[m[2]] ?? 500 } }
    }
  }

  if (allow.has('date')) {
    // 20240315
    if ((m = s.match(new RegExp(`^(.*?)${SEP}(20\\d{2})(\\d{2})(\\d{2})\\s*$`)))) {
      return {
        rest: m[1],
        mark: { kind: 'date', label: `${m[2]}-${m[3]}-${m[4]}`, order: Number(m[2] + m[3] + m[4]) },
      }
    }
    // 2024-03-15 / 2024.3.15 / 2024年3月15日
    if ((m = s.match(new RegExp(`^(.*?)${SEP}(20\\d{2})[-._年](\\d{1,2})[-._月](\\d{1,2})日?\\s*$`)))) {
      const mm = String(Number(m[3])).padStart(2, '0')
      const dd = String(Number(m[4])).padStart(2, '0')
      return {
        rest: m[1],
        mark: { kind: 'date', label: `${m[2]}-${mm}-${dd}`, order: Number(m[2] + mm + dd) },
      }
    }
  }

  return null
}

function stripLoop(base: string, allow: Set<MarkKind>): { rest: string; marks: Mark[] } {
  const marks: Mark[] = []
  let rest = base
  for (let guard = 0; guard < 8; guard++) {
    const hit = takeTrailingMark(rest, allow)
    if (!hit) break
    rest = hit.rest
    marks.push(hit.mark)
  }
  return { rest: rest.replace(new RegExp(`${SEP}$`), '').trim(), marks }
}

const ALL_MARKS: Set<MarkKind> = new Set(['ver', 'copy', 'date'])
const DISPLAY_MARKS: Set<MarkKind> = new Set(['ver', 'copy'])

/** 只留主名和版本痕迹；扩展名由调用方从原文件名取（主名里可能有小数点） */
export function parseName(name: string): Parsed {
  const dot = name.lastIndexOf('.')
  const raw = dot > 0 ? name.slice(0, dot) : name

  // 前缀「副本/copy of」纯噪音，剥掉但不记痕迹——真要区分，后面的撞名处理会补副本N
  const base = raw.replace(/^\s*(副本|拷贝|copy\s*of|copy)\s*/i, '')

  const display = stripLoop(base, DISPLAY_MARKS)
  const full = stripLoop(base, ALL_MARKS)
  const key = full.rest
    .toLowerCase()
    .replace(/（/g, '(')
    .replace(/）/g, ')')
    .replace(/[\s_\-—·:：,，。.、/\\|]/g, '')

  return { key, title: display.rest, marks: display.marks }
}

function extOf(name: string): string {
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(dot).toLowerCase() : ''
}

/** 同扩展的一家人：文档能互换成 pdf，表格/幻灯各成一路；代码/图片不算同一份东西 */
const KIND_OF: Record<string, string> = {
  doc: 'text', docx: 'text', wps: 'text', rtf: 'text', txt: 'text', md: 'text', ofd: 'text',
  pdf: 'text',
  xls: 'sheet', xlsx: 'sheet', csv: 'sheet', et: 'sheet',
  ppt: 'slide', pptx: 'slide', dps: 'slide',
}

function sameKind(a: string, b: string): boolean {
  const x = a.replace(/^\./, '')
  const y = b.replace(/^\./, '')
  if (x === y) return true
  // pdf 常是别的格式导出的盖章/定稿件，两边都允许
  if (x === 'pdf' || y === 'pdf') return true
  const kx = KIND_OF[x]
  return !!kx && kx === KIND_OF[y]
}

/** 一族的「最新」：先看修改时间，再看大小，最后按 id 定死，保证结果稳定 */
function newer(a: FamFile, b: FamFile): number {
  return b.mtime - a.mtime || b.size - a.size || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
}

/**
 * 归族。只返回成员 ≥2 的族——单份文件在界面上不该出现任何折叠痕迹。
 * 调用方通常传「同一个事项目录下的文件」，这样不同目录里的同名文件不会被错并。
 */
export function buildFamilies<T extends FamFile>(files: T[]): Family<T>[] {
  const groups = new Map<string, T[]>()
  for (const f of files) {
    const p = parseName(f.name)
    // 主名太短（(1).docx 这种）说明文件名里没有可辨识的信息，不参与归族
    if ([...p.key].length < 2) continue
    // 同主名但不是同一类东西（html/js/css、drawio/jpg）→ 分到不同桶，各算各的
    const bucket = `${p.key}\u0000${KIND_OF[extOf(f.name).replace(/^\./, '')] ?? extOf(f.name)}`
    const arr = groups.get(bucket)
    if (arr) arr.push(f)
    else groups.set(bucket, [f])
  }

  const out: Family<T>[] = []
  for (const arr of groups.values()) {
    if (arr.length < 2) continue
    const sorted = [...arr].sort(newer)
    // 桶里还可能混着 pdf 和 docx 这种「同类不同扩展」，再按扩展相容性筛一遍
    const keep: T[] = []
    for (const f of sorted) {
      if (keep.every((k) => sameKind(extOf(k.name), extOf(f.name)))) keep.push(f)
    }
    if (keep.length < 2) continue
    const key = parseName(keep[0].name).key
    out.push({ key, title: parseName(keep[0].name).title, files: keep, latestId: keep[0].id })
  }
  return out
}

/**
 * 规范化文件名：返回 id → 新名字，只包含真正需要改的。
 *
 * 规则（每条都有真实文件兜着）：
 *  · 版本痕迹统一挪到末尾括号：`…方案V1.1.docx` → `…方案 (v1.1).docx`
 *  · 没有痕迹的那份保留原名：`…方案.docx` 不动
 *  · 日期留在原位，不挪不改：`…项目组-2026.8.4.pptx` 原样
 *  · 撞一起了就补 `(副本2)`：两份都叫 `ppt修改.pptx` 时，旧的那份变 `ppt修改 (副本2).pptx`
 */
export function familyRenames<T extends FamFile>(files: T[]): Map<string, string> {
  const out = new Map<string, string>()
  for (const fam of buildFamilies(files)) {
    const taken = new Set<string>()
    for (const f of fam.files) {
      const dot = f.name.lastIndexOf('.')
      const ext = dot > 0 ? f.name.slice(dot) : ''
      const p = parseName(f.name)
      const suffix = p.marks.map((m) => m.label).join('-')
      let base = suffix ? `${p.title} (${suffix})` : p.title
      if (taken.has(`${base}${ext}`.toLowerCase())) {
        // 已经是「(副本N)」就把 N 往上顶，别叠成「(副本2) (副本2)」
        const copied = base.match(/^(.*)\(副本(\d+)\)$/)
        if (copied) {
          let n = Number(copied[2])
          do {
            n++
          } while (taken.has(`${copied[1]}(副本${n})${ext}`.toLowerCase()))
          base = `${copied[1]}(副本${n})`
        } else {
          let n = 2
          while (taken.has(`${base} (副本${n})${ext}`.toLowerCase())) n++
          base = `${base} (副本${n})`
        }
      }
      const name = `${base}${ext}`
      taken.add(name.toLowerCase())
      if (name !== f.name) out.set(f.id, name)
    }
  }
  return out
}

export interface FoldRow<T extends FamFile = FamFile> {
  /** 有值 = 这一行是一个版本族，界面折叠显示，点开看 files */
  family?: Family<T>
  /** 这一行要显示的文件：族是全部成员，单份就是它自己 */
  files: T[]
  latest: T
}

/**
 * 把一串文件折成「一行一族」。
 * 保持原顺序：族落在它第一个成员原本的位置上，不因为折叠而让列表跳来跳去。
 */
export function foldFamilies<T extends FamFile>(files: T[]): FoldRow<T>[] {
  const fams = new Map<string, Family<T>>()
  for (const f of buildFamilies(files)) {
    // 同一个 key 可能因扩展相容性被拆成两族，只取成员最多的那族参与折叠
    const old = fams.get(f.key)
    if (!old || f.files.length > old.files.length) fams.set(f.key, f)
  }

  const used = new Set<string>()
  const rows: FoldRow<T>[] = []
  for (const f of files) {
    const key = parseName(f.name).key
    const fam = fams.get(key)
    if (!fam || !fam.files.some((x) => x.id === f.id)) {
      rows.push({ files: [f], latest: f })
      continue
    }
    if (used.has(key)) continue
    used.add(key)
    rows.push({ family: fam, files: fam.files, latest: fam.files[0] })
  }
  return rows
}

/** 这一族里有没有真正的版本号（决定界面写「N 个版本」还是「N 份同名文件」） */
export function hasVersionMark<T extends FamFile>(fam: Family<T>): boolean {
  return fam.files.some((f) => parseName(f.name).marks.some((m) => m.kind === 'ver'))
}

/**
 * 只留每个版本族的最新一份（其余文件照样归档，只是不参与「哪些词算事项」的统计）。
 *
 * 为什么必须这样：df 数的是「出现在多少份文件里」。同一份 PPT 存了 7 个版本，
 * 它名字里的词就被数了 7 次——实测 `海油发展智能工厂` 因此从 10 份虚高到 51 份，
 * 直接把「给这个 PPT 建一个类目」顶到第一名。
 * 实测（455 份真实文件）：去重后归类 304 → 318，未归类 33.2% → 30.1%。
 */
export function latestOnly<T extends FamFile>(files: T[]): T[] {
  const old = new Set<string>()
  for (const fam of buildFamilies(files)) {
    for (const f of fam.files.slice(1)) old.add(f.id)
  }
  return old.size ? files.filter((f) => !old.has(f.id)) : files
}

export interface HashFile {
  id: string
  /** SHA-256；没算过就是空 */
  hash?: string
}

/**
 * 字节完全相同的副本（同一份文件的多个复制品，不是多个版本）。
 *
 * 只标注、不删：用户明确要求「全部归档，只在预览里标注」。
 * 归档后它们仍各占一个文件，但界面上要让人看出「这两份一模一样」。
 */
export function duplicateGroups<T extends HashFile>(files: T[]): Map<string, T[]> {
  const byHash = new Map<string, T[]>()
  for (const f of files) {
    if (!f.hash) continue
    const arr = byHash.get(f.hash)
    if (arr) arr.push(f)
    else byHash.set(f.hash, [f])
  }

  const out = new Map<string, T[]>()
  for (const arr of byHash.values()) {
    if (arr.length < 2) continue
    for (const f of arr) out.set(f.id, arr.filter((x) => x.id !== f.id))
  }
  return out
}
