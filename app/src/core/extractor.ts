import JSZip from 'jszip'
import type { FileKind } from './types'

/** 扩展名 → 文件类型 */
const EXT_KIND: Record<string, FileKind> = {
  doc: 'doc',
  docx: 'doc',
  wps: 'doc',
  xls: 'xls',
  xlsx: 'xls',
  csv: 'xls',
  ppt: 'ppt',
  pptx: 'ppt',
  pdf: 'pdf',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  bmp: 'image',
  webp: 'image',
  gif: 'image',
  drawio: 'drawio',
  txt: 'text',
  md: 'text',
  log: 'text',
  json: 'text',
  yml: 'text',
  yaml: 'text',
  xml: 'text',
  ts: 'code',
  js: 'code',
  py: 'code',
  java: 'code',
  go: 'code',
  rs: 'code',
  vue: 'code',
  sql: 'code',
}

export function extOf(name: string): string {
  const i = name.lastIndexOf('.')
  return i > 0 ? name.slice(i + 1).toLowerCase() : ''
}

export function kindOf(name: string): FileKind {
  return EXT_KIND[extOf(name)] ?? 'other'
}

/** 去掉 XML/HTML 标签，保留纯文本 */
export function stripTags(input: string): string {
  if (!input) return ''
  return input
    .replace(/<br\s*\/?>/gi, ' ')
    .replace(/<\/?[^>]+>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#(\d+);/g, (_, d) => String.fromCharCode(Number(d)))
    .replace(/\s+/g, ' ')
    .trim()
}

export interface DrawioFeature {
  nodes: string[]
  error: boolean
}

function collectNodes(doc: Document): string[] {
  const out: string[] = []
  doc.querySelectorAll('mxCell').forEach((el) => {
    const v = el.getAttribute('value')
    if (v) out.push(stripTags(v))
  })
  doc.querySelectorAll('UserObject').forEach((el) => {
    const l = el.getAttribute('label')
    if (l) out.push(stripTags(l))
  })
  doc.querySelectorAll('object').forEach((el) => {
    const l = el.getAttribute('label')
    if (l) out.push(stripTags(l))
  })
  return out.filter((t) => t.length >= 1 && t.length <= 60 && !/^\d+$/.test(t))
}

/** inflate 一段 base64 编码的 raw-deflate 数据（draw.io 压缩格式） */
async function inflateBase64(b64: string): Promise<string> {
  const DS = (globalThis as unknown as { DecompressionStream?: unknown })
    .DecompressionStream
  if (!DS) throw new Error('DecompressionStream unsupported')
  const bin = atob(b64.replace(/\s+/g, ''))
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  const stream = new Blob([bytes])
    .stream()
    .pipeThrough(new (DS as new (fmt: string) => DecompressionStream)('deflate-raw'))
  const buf = await new Response(stream).arrayBuffer()
  return new TextDecoder('utf-8').decode(buf)
}

/**
 * 解析 draw.io 文件，抽取图内所有节点文字（★差异化能力）
 * 支持三种格式：未压缩 XML、base64+deflate 压缩、URI 编码明文
 */
export async function parseDrawio(xml: string): Promise<DrawioFeature> {
  try {
    const doc = new DOMParser().parseFromString(xml, 'text/xml')
    if (doc.querySelector('parsererror')) {
      // 也可能是被 URI 编码的明文
      try {
        const decoded = decodeURIComponent(xml)
        const doc2 = new DOMParser().parseFromString(decoded, 'text/xml')
        const n2 = collectNodes(doc2)
        if (n2.length) return { nodes: dedupe(n2), error: false }
      } catch {
        /* ignore */
      }
      return { nodes: [], error: true }
    }

    // 1) 未压缩：直接取
    let nodes = collectNodes(doc)
    if (nodes.length) return { nodes: dedupe(nodes), error: false }

    // 2) 压缩：<diagram> 文本是 base64(raw deflate)
    for (const d of Array.from(doc.querySelectorAll('diagram'))) {
      const raw = (d.textContent || '').trim()
      if (!raw) continue
      try {
        const inflated = await inflateBase64(raw)
        let payload = inflated
        try {
          payload = decodeURIComponent(inflated)
        } catch {
          /* 有些版本不二次编码 */
        }
        const doc2 = new DOMParser().parseFromString(payload, 'text/xml')
        nodes = nodes.concat(collectNodes(doc2))
      } catch {
        /* 试下一个 diagram */
      }
    }
    if (nodes.length) return { nodes: dedupe(nodes), error: false }

    return { nodes: [], error: true }
  } catch {
    return { nodes: [], error: true }
  }
}

function dedupe(arr: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const s of arr) {
    const k = s.trim()
    if (k && !seen.has(k)) {
      seen.add(k)
      out.push(k)
    }
  }
  return out
}

/** 从 Office OpenXML（zip）里抽正文 */
async function extractOfficeText(file: File, kind: FileKind): Promise<string> {
  try {
    const zip = await JSZip.loadAsync(await file.arrayBuffer())
    if (kind === 'doc') {
      const entry = zip.file('word/document.xml')
      if (!entry) return ''
      const xml = await entry.async('string')
      return stripTags(xml.replace(/<\/w:p>/g, '\n')).slice(0, 6000)
    }
    if (kind === 'xls') {
      const shared = zip.file('xl/sharedStrings.xml')
      let text = ''
      if (shared) {
        const xml = await shared.async('string')
        text = stripTags(xml.replace(/<\/si>/g, '\n'))
      }
      if (!text) {
        const sheet = zip.file('xl/worksheets/sheet1.xml')
        if (sheet) text = stripTags(await sheet.async('string'))
      }
      return text.slice(0, 6000)
    }
    if (kind === 'ppt') {
      const names = Object.keys(zip.files).filter((n) =>
        /^ppt\/slides\/slide\d+\.xml$/.test(n)
      )
      const parts: string[] = []
      for (const n of names.slice(0, 20)) {
        const xml = await zip.file(n)!.async('string')
        parts.push(stripTags(xml))
      }
      return parts.join(' ').slice(0, 6000)
    }
  } catch {
    return ''
  }
  return ''
}

const PLAIN_TEXT_EXT = new Set([
  'txt', 'md', 'csv', 'log', 'json', 'yml', 'yaml', 'xml', 'ini', 'ts', 'js',
  'py', 'java', 'go', 'rs', 'vue', 'sql', 'html', 'css',
])

/** 抽取文件正文（真实模式） */
export async function extractText(
  file: File,
  kind: FileKind
): Promise<{ text: string; drawioNodes?: string[]; parseError?: boolean }> {
  const ext = extOf(file.name)
  try {
    if (PLAIN_TEXT_EXT.has(ext)) {
      const text = await file.slice(0, 200_000).text()
      return { text: text.slice(0, 6000) }
    }
    if (kind === 'drawio') {
      const xml = await file.text()
      const r = await parseDrawio(xml)
      return {
        text: r.nodes.join(' '),
        drawioNodes: r.nodes,
        parseError: r.error,
      }
    }
    if (kind === 'doc' || kind === 'xls' || kind === 'ppt') {
      return { text: await extractOfficeText(file, kind) }
    }
  } catch {
    return { text: '' }
  }
  return { text: '' }
}
