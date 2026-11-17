import { ALL_SAMPLES } from './sample'
import { extOf } from './extractor'
import type { FileItem, ScanProgress } from './types'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/**
 * 演示模式扫描：使用内置样本模拟一次全盘扫描
 * 用途：浏览器环境无法获得文件系统权限时，仍可完整体验整个流程
 */
export async function demoScan(
  onProgress?: (p: ScanProgress) => void,
  signal?: { cancelled: boolean }
): Promise<FileItem[]> {
  const total = ALL_SAMPLES.length
  const out: FileItem[] = []
  const now = Date.now()

  for (let i = 0; i < ALL_SAMPLES.length; i++) {
    if (signal?.cancelled) break
    const s = ALL_SAMPLES[i]
    out.push({
      id: `d${i}`,
      name: s.name,
      ext: extOf(s.name),
      kind: s.kind,
      size: s.size * 1024,
      mtime: now - s.daysAgo * 86400000,
      path: `D:\\未整理文件\\${s.name}`,
      textSummary: s.content,
      drawioNodes: s.drawioNodes,
      categoryId: null,
      suggestedName: '',
      tags: [],
      confidence: 0,
      needReview: true,
      status: 'scanned',
    })
    onProgress?.({
      processed: i + 1,
      total,
      currentPath: `D:\\未整理文件\\${s.name}`,
      skipped: 0,
    })
    await sleep(22 + Math.random() * 28)
  }
  return out
}
