/** 单词调试：打印某文件在各级分类上的真实得分，用于定位误判原因 */
import fs from 'node:fs'
import path from 'node:path'
import { scoreWorkstreams, scoreDocTypes, induce, docText } from '../src/core/classifier'
import type { FileItem, FileKind } from '../src/core/types'

const inv: { files: any[] } = JSON.parse(
  fs.readFileSync(path.join(process.cwd(), 'test', 'inventory.json'), 'utf8')
)

const queries = process.argv.slice(2)
if (!queries.length) {
  console.log('用法: npx tsx test/debug-one.ts 关键词1 关键词2 ...')
  process.exit(0)
}

// 复用真实测试里的抽取逻辑（简化：只读文本类，office 用 jszip）
import JSZip from 'jszip'
function stripTags(s: string) {
  return s.replace(/<br\s*\/?>/gi, ' ').replace(/<\/?[^>]+>/g, ' ').replace(/\s+/g, ' ').trim()
}
const EXT_KIND: Record<string, FileKind> = {
  doc: 'doc', docx: 'doc', xls: 'xls', xlsx: 'xls', csv: 'xls',
  ppt: 'ppt', pptx: 'ppt', pdf: 'pdf', png: 'image', jpg: 'image',
  txt: 'text', md: 'text', drawio: 'drawio',
}
async function extract(p: string, kind: FileKind, size: number): Promise<string> {
  if (size > 30 * 1024 * 1024) return ''
  try {
    if (kind === 'text') return fs.readFileSync(p, 'utf8').slice(0, 4000)
    if (kind === 'doc' || kind === 'xls' || kind === 'ppt') {
      const zip = await JSZip.loadAsync(fs.readFileSync(p))
      if (kind === 'doc') {
        const e = zip.file('word/document.xml')
        return e ? stripTags((await e.async('string')).replace(/<\/w:p>/g, ' ')).slice(0, 4000) : ''
      }
      if (kind === 'xls') {
        const e = zip.file('xl/sharedStrings.xml')
        return e ? stripTags((await e.async('string')).replace(/<\/si>/g, ' ')).slice(0, 4000) : ''
      }
      const names = Object.keys(zip.files).filter((n) => /^ppt\/slides\/slide\d+\.xml$/.test(n))
      const parts: string[] = []
      for (const n of names.slice(0, 8)) parts.push(stripTags(await zip.file(n)!.async('string')))
      return parts.join(' ').slice(0, 4000)
    }
  } catch {
    return ''
  }
  return ''
}

async function main() {
  for (const q of queries) {
    const hits = inv.files.filter((f) => f.name.includes(q)).slice(0, 2)
    if (!hits.length) {
      console.log(`\n未找到: ${q}`)
      continue
    }
    for (const h of hits) {
      const kind = EXT_KIND[h.ext] ?? 'other'
      const text = await extract(h.path, kind, h.size)
      const item = {
        id: h.rel, name: h.name, ext: h.ext, kind, size: h.size,
        mtime: h.mtime_ts * 1000, path: h.rel, textSummary: text,
        categoryId: null, suggestedName: '', tags: [], confidence: 0,
        needReview: false, status: 'scanned',
      } as FileItem

      console.log('\n' + '='.repeat(72))
      console.log(`文件: ${h.name}`)
      console.log(`正文长度: ${text.length}`)
      console.log('-'.repeat(72))

      const { files } = induce([item], { threshold: 0.7 })
      const r = files[0]
      console.log(`结果: ${r.categoryId ?? '未分类'}  conf=${(r.confidence * 100).toFixed(0)}%  ${r.reason}`)

      console.log('\n  工作域得分:')
      for (const s of scoreWorkstreams(item).slice(0, 4)) {
        console.log(`    ${s.name.padEnd(12)} ${s.score.toFixed(2).padStart(6)}  core=${s.coreHits} strong=${s.strongHits}`)
      }
      console.log('  类型得分:')
      for (const s of scoreDocTypes(item).slice(0, 4)) {
        console.log(`    ${s.name.padEnd(12)} ${s.score.toFixed(2).padStart(6)}  core=${s.coreHits} strong=${s.strongHits}`)
      }
      if (text) console.log(`\n  正文前 200 字: ${docText(item).slice(0, 200)}`)
    }
  }
}
main()
