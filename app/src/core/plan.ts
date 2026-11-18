import type { Category, FileItem, MigrationPlan } from './types'
import { familyRenames } from './version'

/**
 * 迁移计划：决定每个文件最终落到哪个目录、叫什么名字。
 *
 * 这段逻辑原来是写在 MigrateStep.vue 的 computed 里的，没有任何测试覆盖——
 * 但它正是「永不覆盖」这条底线的实现处：同名必须自动加序号，且序号不能撞车。
 * 抽出来之后既能测，也能被别处（如预览页、日志回放）复用。
 */

/** 与 Rust 侧 migrate::sanitize_segment 保持一致，保证预览显示的路径就是实际会建的路径 */
const ILLEGAL = /[\\/:*?"<>|]/g
const CONTROL = /[\u0000-\u001f]/g

export function sanitizeSegment(s: string): string {
  return s.replace(ILLEGAL, '_').replace(CONTROL, '_').trim().replace(/\.+$/, '')
}

/** 分类在归档目录里的相对路径；未分类返回空串 */
function dirOf(cat: Category | undefined, byId: Map<string, Category>): string {
  if (!cat) return ''
  const parent = cat.parentId ? byId.get(cat.parentId) : undefined
  const segs = parent ? [parent.name, cat.name] : [cat.name]
  return segs.map(sanitizeSegment).filter(Boolean).join('\\')
}

/** 同名加序号：纪要.docx → 纪要_1.docx，没有扩展名则整体加后缀 */
function withIndex(name: string, n: number): string {
  const dot = name.lastIndexOf('.')
  if (dot > 0) return `${name.slice(0, dot)}_${n}${name.slice(dot)}`
  return `${name}_${n}`
}

export function buildPlans(files: FileItem[], categories: Category[]): MigrationPlan[] {
  const byId = new Map(categories.map((c) => [c.id, c]))
  const entries = files.map((f) => {
    const cat = f.categoryId ? byId.get(f.categoryId) : undefined
    return {
      f,
      dirName: dirOf(cat, byId),
      // 待确认和服务没归类的都不动，宁可不整理也不能乱放
      deferred: !cat || f.needReview,
    }
  })

  // 版本族只在本目录内成立：A 事项下的「方案V1」和 B 事项下的「方案V2」是两份不同的东西。
  // 用 AI 建议名的文件不参与——名字是人定的，不该被机器再改一遍。
  const buckets = new Map<string, FileItem[]>()
  for (const e of entries) {
    if (e.deferred || e.f.suggestedName) continue
    const arr = buckets.get(e.dirName)
    if (arr) arr.push(e.f)
    else buckets.set(e.dirName, [e.f])
  }
  const renamedById = new Map<string, string>()
  for (const list of buckets.values()) {
    if (list.length < 2) continue
    const m = familyRenames(list.map((f) => ({ id: f.id, name: f.name, size: f.size, mtime: f.mtime })))
    for (const [id, name] of m) renamedById.set(id, name)
  }

  /** 已占用的「目录\文件名」，用来给同名文件让位 */
  const seen = new Set<string>()

  return entries.map(({ f, dirName, deferred }) => {
    const familyName = renamedById.get(f.id)
    let targetName = f.suggestedName || familyName || f.name

    let conflict = false
    if (!deferred) {
      if (seen.has(`${dirName}\\${targetName}`)) {
        conflict = true
        let i = 1
        while (seen.has(`${dirName}\\${withIndex(targetName, i)}`)) i++
        targetName = withIndex(targetName, i)
      }
      seen.add(`${dirName}\\${targetName}`)
    }

    return {
      fileId: f.id,
      fileName: f.name,
      sourcePath: f.path,
      // 类名被清洗成空串时（极端情况）退回归档根目录
      targetDir: dirName || '（未分类）',
      targetName,
      conflict,
      renamed: !deferred && !!familyName,
      deferred,
    }
  })
}
