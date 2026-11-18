/**
 * 方案存档：把用户确认过的分类方案存下来，下次扫描可以直接沿用。
 *
 * 为什么需要：
 *   涌现出的分类方案是用户一个个删改出来的（改名、合并、删垃圾类），
 *   关掉程序就全没了，下次扫描要从头再调一遍——这是这版最伤的体验断点。
 *
 * 为什么存「类名 + 别名」而不是存文件 id：
 *   每次扫描文件 id 都是新生成的，存 id 一定对不上。
 *   类名/别名能重新去匹配新一批文件的文件名和正文，于是：
 *     · 同目录重扫 → 类目完全一致，直接跳过模型（快且没有惊喜）
 *     · 换一批新文件 → 新文件自动落进老类目，这才是「掌控全局」该有的样子
 */
import type { FileItem } from '../types'
import { reassign, type Proposal, type ProposalTopic } from './proposal'

/** 存档结构变了就+1，旧存档会被当成「没有」，不冒险解释 */
export const ARCHIVE_VERSION = 2
/** 还能读的旧版本：v1 没有 domain 字段，读出来是一层目录，其余照旧可用 */
const READABLE_VERSIONS = [1, ARCHIVE_VERSION]

export interface SavedTopic {
  /** 用户确认过的类名（可能被改名过） */
  name: string
  /** 涌现出来的原始写法，用于匹配文件名 */
  aliases: string[]
  /** 所属大域（归档第一层）。v1 存档没有这个字段 */
  domain?: string
  weak?: boolean
  weakReason?: 'doc' | 'who'
}

export interface SavedPlan {
  version: number
  /** 归档根目录。换根目录时提示用户「这是另一套方案」 */
  root: string
  savedAt: number
  topics: SavedTopic[]
}

/** 提案 → 存档。只留匹配用得上的信息，文件 id 和样本名一律不留 */
export function toSavedPlan(proposal: Proposal, root: string, savedAt = Date.now()): SavedPlan {
  return {
    version: ARCHIVE_VERSION,
    root,
    savedAt,
    topics: proposal.topics.map((t) => ({
      name: t.name,
      aliases: [...t.aliases],
      domain: t.domain,
      weak: t.weak,
      weakReason: t.weakReason,
    })),
  }
}

/**
 * 存档 → 提案类目。
 *
 * 任何一处不合规都只丢那一条，不整份作废——用户的存档可能是上个版本写的，
 * 宁可少一个类，也不能让整个流程起不来。
 */
export function fromSavedPlan(saved: SavedPlan | null | undefined): ProposalTopic[] | null {
  if (!saved || !READABLE_VERSIONS.includes(saved.version)) return null
  if (!Array.isArray(saved.topics) || saved.topics.length === 0) return null

  const topics: ProposalTopic[] = []
  for (const t of saved.topics) {
    const name = (t?.name ?? '').trim()
    if (!name) continue
    const aliases = (Array.isArray(t.aliases) ? t.aliases : []).map((a) => String(a).trim()).filter(Boolean)
    // 只认字符串：脏数据（数字/null）当成没有大类，不要变成 "123" 这样的目录名
    const domain = typeof t.domain === 'string' ? t.domain.trim() : ''
    topics.push({
      // 存档里的类目重新编号，避免和本轮新生成的 t1/t2 撞车
      id: `s${topics.length + 1}`,
      name,
      // 别名缺失时退回类名，否则这个类永远匹配不到任何文件
      aliases: aliases.length ? aliases : [name],
      fileIds: [],
      samples: [],
      domain: domain || undefined,
      weak: t.weak,
      weakReason: t.weakReason,
    })
  }
  return topics.length ? topics : null
}

/**
 * 沿用存档：按类名/别名重新认领这一批文件。
 * 返回 null 表示存档不可用（版本不符 / 空），调用方应回落到「重新生成」。
 */
export function buildProposalFromSaved(files: FileItem[], saved: SavedPlan): Proposal | null {
  const topics = fromSavedPlan(saved)
  if (!topics) return null
  const p = reassign(files, topics)
  // reassign 为编辑场景保留了空类（用户刚删完文件时类还在界面上），
  // 但沿用存档不该带进僵尸目录：一个文件都没认领到的类直接丢掉
  return { topics: p.topics.filter((t) => t.fileIds.length > 0), ungroupedIds: p.ungroupedIds }
}
