/**
 * 真实文件分类测试（只读）
 * 用真实的 classifier.ts 跑真实桌面文件，检验引擎在你实际数据上的表现
 */
import fs from 'node:fs'
import path from 'node:path'
import JSZip from 'jszip'
import { induce } from '../src/core/classifier'
import type { FileItem, FileKind } from '../src/core/types'

const INVENTORY = path.join(process.cwd(), 'test', 'inventory.json')
const CONTENT_LIMIT = 30 * 1024 * 1024 // 超过 30MB 不读正文

const EXT_KIND: Record<string, FileKind> = {
  doc: 'doc', docx: 'doc', wps: 'doc',
  xls: 'xls', xlsx: 'xls', csv: 'xls',
  ppt: 'ppt', pptx: 'ppt',
  pdf: 'pdf', png: 'image', jpg: 'image', jpeg: 'image',
  bmp: 'image', webp: 'image', gif: 'image',
  drawio: 'drawio', txt: 'text', md: 'text', html: 'text',
}

function stripTags(s: string): string {
  return s
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(/<\/?[^>]+>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/\s+/g, ' ')
    .trim()
}

/** 抽正文（Node 版，与浏览器 extractor 同策略） */
async function extract(filePath: string, kind: FileKind, size: number): Promise<string> {
  if (size > CONTENT_LIMIT) return ''
  try {
    if (kind === 'text') {
      return fs.readFileSync(filePath, 'utf8').slice(0, 4000)
    }
    if (kind === 'drawio') {
      const raw = fs.readFileSync(filePath, 'utf8')
      const m = raw.match(/(?:value|label)="([^"]{1,60})"/g) ?? []
      return m.map((x) => x.replace(/^[a-z]+="/i, '').replace(/"$/, '')).join(' ')
    }
    if (kind === 'doc' || kind === 'xls' || kind === 'ppt') {
      const buf = fs.readFileSync(filePath)
      const zip = await JSZip.loadAsync(buf)
      if (kind === 'doc') {
        const e = zip.file('word/document.xml')
        if (!e) return ''
        const xml = await e.async('string')
        return stripTags(xml.replace(/<\/w:p>/g, ' ')).slice(0, 4000)
      }
      if (kind === 'xls') {
        const e = zip.file('xl/sharedStrings.xml')
        if (!e) return ''
        return stripTags((await e.async('string')).replace(/<\/si>/g, ' ')).slice(0, 4000)
      }
      const names = Object.keys(zip.files).filter((n) => /^ppt\/slides\/slide\d+\.xml$/.test(n))
      const parts: string[] = []
      for (const n of names.slice(0, 12)) {
        parts.push(stripTags(await zip.file(n)!.async('string')))
      }
      return parts.join(' ').slice(0, 4000)
    }
  } catch {
    return ''
  }
  return ''
}

interface Inv {
  path: string
  rel: string
  name: string
  ext: string
  size: number
  size_h: string
  mtime: string
  mtime_ts: number
  kind: string
}

async function main() {
  const inv: { target: string; files: Inv[] } = JSON.parse(fs.readFileSync(INVENTORY, 'utf8'))
  console.log(`读取清单: ${inv.files.length} 个文件\n`)
  console.log('正在抽取正文（大文件跳过正文）…')

  const items: (FileItem & { _rel: string; _sizeH: string })[] = []
  let done = 0
  let withText = 0

  for (const f of inv.files) {
    const kind = EXT_KIND[f.ext] ?? 'other'
    const text = await extract(f.path, kind, f.size)
    if (text.length > 20) withText++
    items.push({
      id: f.rel,
      name: f.name,
      ext: f.ext,
      kind,
      size: f.size,
      mtime: f.mtime_ts * 1000,
      path: f.rel,
      textSummary: text,
      categoryId: null,
      suggestedName: '',
      tags: [],
      confidence: 0,
      needReview: false,
      status: 'scanned',
      _rel: f.rel,
      _sizeH: f.size_h,
    } as FileItem & { _rel: string; _sizeH: string })

    done++
    if (done % 60 === 0) process.stdout.write(`  ${done}/${inv.files.length}\r`)
  }
  console.log(`  完成：${done} 个，其中 ${withText} 个成功抽到正文\n`)

  // ---- 跑真实分类引擎 ----
  const t0 = Date.now()
  const { result, files } = induce(items, { threshold: 0.7 })
  const cost = Date.now() - t0

  console.log('='.repeat(74))
  console.log('【真实分类引擎在你桌面 450 个文件上的结果】')
  console.log('='.repeat(74))
  console.log(`耗时 ${cost}ms  |  归纳出 ${result.categories.filter((c) => !c.parentId).length} 个工作域`)

  // ---- 分层树状展示（一级工作域 → 二级文档类型）----
  const catMap = new Map(result.categories.map((c) => [c.id, c]))
  const parentOf = (id: string | null) => {
    const c = id ? catMap.get(id) : undefined
    if (!c) return null
    return c.parentId ? (catMap.get(c.parentId) ?? null) : c
  }
  // 文件按"一级分类 + 二级分类"分桶
  const bucket = new Map<string, { ws: string; dt: string | null; files: FileItem[] }>()
  for (const f of files) {
    const dt = f.categoryId ? catMap.get(f.categoryId) : undefined
    const ws = parentOf(f.categoryId)
    const key = ws ? `${ws.name}||${dt && dt.parentId ? dt.name : ''}` : '__none__'
    if (!bucket.has(key)) {
      bucket.set(key, {
        ws: ws?.name ?? '（未分类）',
        dt: dt && dt.parentId ? dt.name : null,
        files: [],
      })
    }
    bucket.get(key)!.files.push(f)
  }

  console.log('\n【分类树（一级工作域 → 二级文档类型 · 文件数）】')
  const wsGroups = new Map<string, { files: FileItem[]; subs: [string, number][] }>()
  for (const b of bucket.values()) {
    if (b.ws === '（未分类）') continue
    if (!wsGroups.has(b.ws)) wsGroups.set(b.ws, { files: [], subs: [] })
    const g = wsGroups.get(b.ws)!
    g.files.push(...b.files)
    if (b.dt) g.subs.push([b.dt, b.files.length])
    else g.subs.push(['(未细分)', b.files.length])
  }
  for (const [ws, g] of [...wsGroups.entries()].sort((a, b) => b[1].files.length - a[1].files.length)) {
    console.log(`  ▸ ${ws.padEnd(14)} ${String(g.files.length).padStart(4)} 个`)
    for (const [dt, n] of g.subs.sort((a, b) => b[1] - a[1])) {
      console.log(`      └ ${dt.padEnd(12)} ${String(n).padStart(3)}`)
    }
  }
  const unclassified = files.filter((f) => !f.categoryId)
  console.log(`  ▸ （未分类）         ${String(unclassified.length).padStart(4)} 个  ← 需人工处理`)

  const high = files.filter((f) => f.confidence >= 0.85).length
  const mid = files.filter((f) => f.confidence >= 0.7 && f.confidence < 0.85).length
  const low = files.filter((f) => f.confidence < 0.7).length
  console.log(`\n【置信度分布】高(≥85%) ${high}  |  中(70-85%) ${mid}  |  低(<70%) ${low}`)
  console.log(`平均置信度: ${(files.reduce((a, f) => a + f.confidence, 0) / files.length).toFixed(3)}`)
  console.log(`自动通过（conf≥0.7 且有分类）: ${files.filter((f) => !f.needReview).length} 个`)
  console.log(`进待确认队列: ${files.filter((f) => f.needReview).length} 个`)

  const fullPath = (f: FileItem) => {
    const dt = f.categoryId ? catMap.get(f.categoryId) : undefined
    const ws = parentOf(f.categoryId)
    if (!ws) return '—（未分类）'
    return dt && dt.parentId ? `${ws.name} › ${dt.name}` : ws.name
  }

  // ---- 抽样明细：让用户自己判断对不对 ----
  console.log('\n' + '='.repeat(74))
  console.log('【抽样核对 · 高置信样本（AI 最有把握的 20 个）】')
  console.log('='.repeat(74))
  for (const f of [...files].sort((a, b) => b.confidence - a.confidence).slice(0, 20)) {
    console.log(`\n  ${f.name.slice(0, 58)}`)
    console.log(`    → ${fullPath(f)}  (${(f.confidence * 100).toFixed(0)}%)`)
  }

  console.log('\n' + '='.repeat(74))
  console.log('【未分类明细（AI 明确说"我判断不了"）】')
  console.log('='.repeat(74))
  for (const f of unclassified.slice(0, 20)) {
    console.log(`  [${(f.confidence * 100).toFixed(0).padStart(2)}%] ${f.name.slice(0, 56)}`)
  }
  if (unclassified.length > 20) console.log(`  … 另有 ${unclassified.length - 20} 个`)

  // ---- 导出明细供后续分析 ----
  const out = {
    total: files.length,
    categories: result.categories.map((c) => ({
      name: c.name,
      parent: c.parentId ? (catMap.get(c.parentId)?.name ?? null) : null,
    })),
    dist: [...wsGroups.entries()].map(([ws, g]) => ({ ws, count: g.files.length })),
    conf: { high, mid, low },
    details: files.map((f) => ({
      name: f.name,
      rel: (f as FileItem & { _rel: string })._rel,
      kind: f.kind,
      path: fullPath(f),
      ws: parentOf(f.categoryId)?.name ?? null,
      dt:
        f.categoryId && catMap.get(f.categoryId)?.parentId
          ? (catMap.get(f.categoryId)?.name ?? null)
          : null,
      cat: f.categoryId ? catMap.get(f.categoryId)?.name ?? null : null,
      conf: Number(f.confidence.toFixed(3)),
      needReview: f.needReview,
      reason: f.reason,
      suggested: f.suggestedName,
      textLen: f.textSummary.length,
    })),
  }
  fs.writeFileSync(path.join(process.cwd(), 'test', 'real-classify.json'), JSON.stringify(out, null, 1), 'utf8')
  console.log(`\n明细已写出: test/real-classify.json`)
}

main()
