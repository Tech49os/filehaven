/**
 * 确认页的展示规则（纯函数，与 Vue 无关）。
 *
 * 为什么份数要分档：分类不折叠时，31 行长得一模一样，眼睛没有落点。
 * 把份数做成左侧的大号数字并分三档着色，扫视时先看到「量」、再看「名」，
 * 主次是视觉上分出来的，不是靠隐藏小类分出来的。
 */

/** 份数档位：major ≥10 / normal 4-9 / minor ≤3 */
export type CountTier = 'major' | 'normal' | 'minor'

export function countTier(n: number): CountTier {
  if (n >= 10) return 'major'
  if (n >= 4) return 'normal'
  return 'minor'
}

/**
 * 按份数降序排，份数相同保持原顺序。
 *
 * 为什么要排：reassign 会重算归属但不重排顺序，改名/合并之后可能出现
 * 「2 份的排在 8 份前面」，主次又乱了。排序在展示层做，不动数据。
 */
export function sortForDisplay<T extends { fileIds: readonly unknown[] }>(
  topics: readonly T[]
): T[] {
  return topics
    .map((t, i) => ({ t, i }))
    .sort((a, b) => b.t.fileIds.length - a.t.fileIds.length || a.i - b.i)
    .map((x) => x.t)
}

/** 未归类文件按所在目录成的一组 */
export interface FolderGroup {
  /** 完整目录路径，用于去重 */
  folder: string
  /** 展示用：末两级目录名 */
  label: string
  fileIds: string[]
}

/**
 * 把未归类的文件按所在目录分组。
 *
 * 为什么按目录：这批文件正是因为「文件名里没写清属于哪件事」才没归类，
 * 唯一还剩的线索就是它们躺在哪个文件夹里。按目录摆出来，用户一眼能看出
 * 「原来是某次下载的一大堆」还是「这几个是我漏掉的」，再决定要不要补类。
 *
 * 只显示末两级目录：完整路径太长塞不下，而且前面几级（C:/Users/x/Desktop）
 * 对判断毫无帮助。
 */
export function groupUngrouped(
  ids: readonly string[],
  pathOf: (id: string) => string | undefined
): FolderGroup[] {
  const byFolder = new Map<string, string[]>()
  for (const id of ids) {
    const path = pathOf(id)
    const cut = path ? path.lastIndexOf('\\') : -1
    const folder = cut > 0 ? path!.slice(0, cut) : path ? '' : '\u0000未知'
    const list = byFolder.get(folder)
    if (list) list.push(id)
    else byFolder.set(folder, [id])
  }

  const out: FolderGroup[] = []
  for (const [folder, fileIds] of byFolder) {
    if (folder === '\u0000未知') {
      out.push({ folder: '', label: '未知目录', fileIds })
      continue
    }
    const parts = folder.split('\\').filter(Boolean)
    out.push({
      folder,
      label: parts.length ? parts.slice(-2).join('\\') : '(根目录)',
      fileIds,
    })
  }
  out.sort((a, b) => b.fileIds.length - a.fileIds.length)
  return out
}
