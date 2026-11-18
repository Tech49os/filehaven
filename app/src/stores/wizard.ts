import { defineStore } from 'pinia'
import {
  scanDirectory as scanBrowser,
  supportsFSA,
  pickDirectory as pickBrowserDir,
} from '../core/fs'
import {
  isDesktop,
  pickDirectory as pickDesktopDir,
  hashFiles,
  prepareArchiveRoot,
  scanDirectory as scanDesktop,
  toFileItems,
} from '../core/desktop'
import { demoScan } from '../core/demo'
import { collectCandidates, type Candidate } from '../core/emergence/candidates'
import { groupBySurface } from '../core/emergence/grouping'
import { groupUngrouped, sortForDisplay } from '../core/emergence/view'
import { duplicateGroups, latestOnly } from '../core/version'
import { DOC_KIND_WORDS } from '../core/emergence/lexicon'
import {
  augmentWithOrgRule,
  expandWho,
  type WhoKind,
} from '../core/emergence/whoJudge'
import { judgeWhoWithLlm, judgeDomainsWithLlm } from '../core/llm'
import {
  domainsFromSharedToken,
  isUsable,
  type DomainResult,
} from '../core/emergence/domain'
import {
  applyProposal,
  buildProposal,
  reassign,
  type Proposal,
} from '../core/emergence/proposal'
import {
  buildProposalFromSaved,
  toSavedPlan,
  type SavedPlan,
} from '../core/emergence/archive'
import { dbGet, dbSet } from '../core/db'
import type {
  AppMode,
  FileItem,
  HybridProgress,
  HybridStats,
  InductionResult,
  ScanProgress,
} from '../core/types'
import { useDataStore } from './data'

/** 文件句柄不放进响应式容器（Proxy 会破坏句柄方法） */
let rootHandleRef: FileSystemDirectoryHandle | null = null
let rawFiles: FileItem[] = []

export function getRootHandle(): FileSystemDirectoryHandle | null {
  return rootHandleRef
}

export function getRawFiles(): FileItem[] {
  return rawFiles
}

export function fileHandleMap(): Map<string, FileSystemFileHandle> {
  const m = new Map<string, FileSystemFileHandle>()
  for (const f of rawFiles) if (f.handle) m.set(f.id, f.handle)
  return m
}

/** 归纳过程的实时状态（供进度页展示） */
export interface AiLiveState {
  active: boolean
  phase: HybridProgress['phase'] | ''
  done: number
  total: number
  current: string
  startedAt: number
  stats: HybridStats | null
  /** 最近判完的文件（倒序），边跑边显示 */
  log: Array<NonNullable<HybridProgress['event']>>
}

const emptyAi = (): AiLiveState => ({
  active: false,
  phase: '',
  done: 0,
  total: 0,
  current: '',
  startedAt: 0,
  stats: null,
  log: [],
})

/** 默认归档根目录（用户可在欢迎页更改） */
export const DEFAULT_ARCHIVE = 'D:\\文档归档'

export const useWizardStore = defineStore('wizard', {
  state: () => ({
    step: 1,
    scanning: false,
    inducing: false,
    migrating: false,
    mode: 'demo' as AppMode,
    /** 要整理的源目录（桌面版为绝对路径） */
    sourcePath: '',
    sourceLabel: '未选择目录',
    archivePath: DEFAULT_ARCHIVE,
    progress: { processed: 0, total: 0, currentPath: '', skipped: 0 } as ScanProgress,
    error: '',
    summary: null as InductionResult | null,
    /** 本轮归纳的实时进度 */
    ai: emptyAi(),
    /** 涌现出的分类方案（确认之前不落盘、不建目录） */
    proposal: null as Proposal | null,
    /** 被判定为人名/组织名、因而不进类目的词（确认页可一键恢复） */
    whoExcluded: [] as string[],
    /**
     * 文件 id → 文件名。确认页要能展开某个分类看「到底装了哪些文件」，
     * 而 ProposalTopic.samples 只留了 5 个样本，所以单独存一份索引。
     */
    fileIndex: {} as Record<string, string>,
    /** 文件 id → 完整路径。未归类面板要按目录分组，只有文件名分不出来 */
    filePathIndex: {} as Record<string, string>,
    /**
     * 文件 id → 大小/修改时间。版本族要按修改时间排「最新那份」，
     * 光有文件名判断不了谁是新的。
     */
    fileMeta: {} as Record<string, { size: number; mtime: number }>,
    /**
     * 文件 id → 与它字节完全相同的其他文件 id。
     *
     * 只标注、不删（用户要求「全部归档，只在预览里标注」）。
     * 只对「同尺寸且不止一份」的文件算 SHA-256：真实数据 455 份里只涉及 26 份，
     * 全盘哈希要读几十 GB，不值得。
     */
    dupIndex: {} as Record<string, string[]>,
    /**
     * 上次确认过的方案存档。有它就能跳过「重新生成 + 重调一遍」，
     * 直接沿用同一套类目——这是把工具从「每次重来」变成「越用越顺」的关键。
     */
    savedPlan: null as SavedPlan | null,
    /**
     * 第一层目录（大域）是谁分的：
     *  · llm  模型看这批文件判断「这台电脑在管哪些方向」
     *  · rule 模型不可用，按事项名的公共开头粗略分组
     *  · none 连粗略分组都凑不出来，全部放一级
     */
    domainSource: 'none' as 'llm' | 'rule' | 'none',
    cancelled: { cancelled: false },
    scannedCount: 0,
    skippedCount: 0,
    /** 是否运行在桌面客户端里 */
    desktop: isDesktop,
  }),

  getters: {
    stepName: (s) =>
      [
        '',
        '设置归档位置',
        '扫描文件',
        '生成分类方案',
        '确认分类方案',
        '人工微调',
        '预览迁移',
        '安全迁移',
        '完成',
      ][s.step] ?? '',
    canPrev: (s) => s.step > 1 && s.step < 7,
    /** 确认页用：按文件数降序的稳定排序，空类沉底 */
    proposalTopics: (s) => (s.proposal ? sortForDisplay(s.proposal.topics) : []),
    /** 现有的大域名（左树改名/拖动时给下拉用） */
    domainNames: (s) => {
      const seen: string[] = []
      for (const t of s.proposal?.topics ?? []) {
        const d = (t.domain ?? '').trim()
        if (d && !seen.includes(d)) seen.push(d)
      }
      return seen
    },
    ungroupedCount: (s) => s.proposal?.ungroupedIds.length ?? 0,
    /** 未归类文件按目录成组，确认页据此让用户看清「剩的到底是什么」 */
    ungroupedGroups: (s) =>
      s.proposal
        ? groupUngrouped(s.proposal.ungroupedIds, (id) => s.filePathIndex[id])
        : [],
  },

  actions: {
    /** 选择要整理的目录（桌面版走系统对话框） */
    async selectRealRoot() {
      this.error = ''
      if (isDesktop) {
        const dir = await pickDesktopDir('选择要整理的目录', this.sourcePath || undefined)
        if (!dir) {
          this.error = '未选择目录'
          return false
        }
        this.mode = 'real'
        this.sourcePath = dir
        this.sourceLabel = dir
        // 归档根不能落在源目录内，否则会自我递归
        if (this.archivePath && this.archivePath.toLowerCase().startsWith(dir.toLowerCase())) {
          this.archivePath = DEFAULT_ARCHIVE
        }
        return true
      }

      // 浏览器兜底（开发调试用）
      const h = await pickBrowserDir('readwrite')
      if (h) {
        rootHandleRef = h
        this.mode = 'real'
        this.sourceLabel = h.name
        this.archivePath = `${h.name}\\_已归档`
        return true
      }
      this.mode = 'demo'
      this.sourceLabel = '未选择目录'
      this.error = supportsFSA
        ? '未选择目录，已切换为示例数据模式'
        : '当前环境不支持访问本地文件，已切换为示例数据模式'
      return false
    },

    /** 更改归档根目录 */
    async pickArchiveRoot() {
      if (!isDesktop) return false
      const dir = await pickDesktopDir('选择归档根目录', this.archivePath || undefined)
      if (!dir) return false
      this.archivePath = dir
      return true
    },

    useDemo() {
      rootHandleRef = null
      this.mode = 'demo'
      this.sourcePath = ''
      this.sourceLabel = '示例数据'
      this.archivePath = DEFAULT_ARCHIVE
      this.error = ''
    },

    async startScan() {
      const data = useDataStore()
      this.scanning = true
      this.error = ''
      this.cancelled = { cancelled: false }
      // 重新扫描意味着上一轮的分类结论作废
      this.summary = null
      this.ai = emptyAi()
      this.progress = { processed: 0, total: 0, currentPath: '', skipped: 0 }
      this.skippedCount = 0
      try {
        if (this.mode === 'real' && isDesktop && this.sourcePath) {
          // 桌面版：Rust 侧递归扫描 + 正文抽取
          const res = await scanDesktop(this.sourcePath, {
            archiveRoot: this.archivePath,
            maxSizeMB: data.settings.maxFileSizeMB || 2048,
            limit: 20000,
            onProgress: (p) => {
              this.progress = p
            },
          })
          rawFiles = toFileItems(res.files)
          this.skippedCount = res.skipped
        } else if (this.mode === 'real' && rootHandleRef) {
          rawFiles = await scanBrowser(rootHandleRef, {
            maxSizeBytes: 2048 * 1024 * 1024,
            archiveRoot: this.archivePath,
            onProgress: (p) => {
              this.progress = p
            },
            signal: this.cancelled,
          })
        } else {
          rawFiles = await demoScan((p) => {
            this.progress = p
          }, this.cancelled)
        }
        this.scannedCount = rawFiles.length
        const index: Record<string, string> = {}
        const paths: Record<string, string> = {}
        const meta: Record<string, { size: number; mtime: number }> = {}
        for (const f of rawFiles) {
          index[f.id] = f.name
          paths[f.id] = f.path
          meta[f.id] = { size: f.size, mtime: f.mtime }
        }
        this.fileIndex = index
        this.filePathIndex = paths
        this.fileMeta = meta

        // 字节完全相同的副本：只对「同尺寸且不止一份」的文件算哈希
        this.dupIndex = {}
        if (isDesktop) {
          const bySize = new Map<number, { id: string; path: string }[]>()
          for (const f of rawFiles) {
            if (f.size <= 0) continue
            const arr = bySize.get(f.size)
            if (arr) arr.push({ id: f.id, path: f.path })
            else bySize.set(f.size, [{ id: f.id, path: f.path }])
          }
          const need = [...bySize.values()].filter((v) => v.length > 1)
          if (need.length) {
            try {
              const hashes = await hashFiles(need.flat().map((x) => x.path))
              const dups = duplicateGroups(
                rawFiles.map((f) => ({ id: f.id, hash: hashes[f.path] }))
              )
              const out: Record<string, string[]> = {}
              for (const [id, others] of dups) out[id] = others.map((o) => o.id)
              this.dupIndex = out
            } catch {
              // 哈希失败不影响扫描：只是少了「内容重复」的标注
            }
          }
        }
        if (rawFiles.length === 0) {
          this.error = '这个位置没有找到可整理的文件'
        }
      } catch (e) {
        this.error = e instanceof Error ? e.message : '扫描失败'
      } finally {
        this.scanning = false
      }
      return rawFiles.length
    },

    /**
     * 判定哪些候选是「谁」（人名 / 组织名 / 团队名）。
     *
     * 这是整条流水线里**唯一**交给模型的判断。实测依据见 whoJudge.ts 顶部注释：
     * 让它做归并归纳会崩，只问「是不是人名/组织名」它很准（9b 10/10、0 误伤）。
     * 模型不可用就退回后缀词典，功能不消失，只是会漏掉「海油发展」「集团」这类
     * 没有组织后缀的简称。
     */
    async judgeWho(cands: Candidate[]): Promise<Set<string>> {
      const data = useDataStore()
      const s = data.settings
      let base: Map<string, WhoKind> = new Map()
      if (s.llmEnabled && cands.length > 0) {
        this.ai.current = '正在让本地模型分辨哪些词是人名/组织名…'
        try {
          base = await judgeWhoWithLlm(cands, {
            baseUrl: s.llmBaseUrl,
            model: s.llmModel,
            apiKey: s.llmApiKey,
          })
          this.ai.current = `模型判定了 ${base.size} 个词，正在分组…`
        } catch {
          this.ai.current = '模型没有响应，改用后缀规则识别组织名…'
          base = new Map()
        }
      }
      return expandWho(augmentWithOrgRule(base, cands), cands)
    },

    /**
     * 生成分类方案：从文件名里涌现事项 → 保守分组 → 装配归属提案
     *
     * 全程纯 CPU（几百毫秒），不调模型、不写文件。产出只是一份「提案」，
     * 用户在第 4 步确认之后才真正建目录。
     */
    async runProposal() {
      this.inducing = true
      this.error = ''
      this.summary = null
      this.proposal = null
      this.whoExcluded = []
      this.cancelled = { cancelled: false }
      this.ai = {
        active: true,
        phase: 'rules',
        done: 0,
        total: rawFiles.length,
        current: '正在从文件名里抽取事项…',
        startedAt: Date.now(),
        stats: null,
        log: [],
      }
      try {
        const files = rawFiles
        if (files.length === 0) {
          this.error = '没有可归纳的文件'
          return
        }
        // 同一份文档的多个版本只贡献一次统计：否则「存了 7 个版本的 PPT」名字里的词
        // 会被数 7 遍，直接垄断候选榜（实测 455 份上去重后归类 304 → 318）
        const forStats = latestOnly(
          files.map((f) => ({ id: f.id, name: f.name, size: f.size, mtime: f.mtime }))
        )
        const statIds = new Set(forStats.map((f) => f.id))
        const cands = collectCandidates(
          files.filter((f) => statIds.has(f.id)).map((f) => ({ name: f.name, path: f.path })),
          { topK: 60 }
        )
        // 纯英文短词（ppt/work）和纯格式词（演讲稿/申请表）不当事项，否则确认页全是垃圾
        const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
        const useful = cands.filter((c) => cn(c.term) >= 2 && !DOC_KIND_WORDS.has(c.term))

        // 哪些词是「谁」（人名/组织名）—— 唯一交给模型的一步，模型挂了就用后缀规则兜
        const who = await this.judgeWho(useful)
        this.whoExcluded = useful.filter((c) => who.has(c.term)).map((c) => c.term)

        const groups = groupBySurface(useful, { who }).filter((g) => g.weight >= 3)

        const p = buildProposal(files, groups)
        // 第一层目录：模型判断「这台电脑在管哪些方向」，不行就按名字公共开头粗分
        await this.assignDomains(p)
        this.proposal = p
        this.ai.phase = 'done'
        this.ai.done = files.length
        this.ai.total = files.length
        const doms = new Set(p.topics.map((t) => t.domain).filter(Boolean)).size
        this.ai.current = doms
          ? `归纳出 ${doms} 个大类、${p.topics.length} 个事项，${p.ungroupedIds.length} 份未归类`
          : `归纳出 ${p.topics.length} 个事项，${p.ungroupedIds.length} 份未归类`
      } catch (e) {
        this.error = e instanceof Error ? e.message : '生成方案失败'
      } finally {
        this.inducing = false
        this.ai.active = false
      }
    },

    /**
     * 给事项分域（第一层目录）。
     *
     * 只有「正经事项」参与：兜底类（人名/单位/仅文种）本来就回答不了
     * 「哪件事」，塞给模型只会污染域划分。
     * 模型结果不可用（说得太少、几乎全塞进一个域）就不采信，退回公共开头分组，
     * 界面会注明这是粗略分组——宁可承认粗糙，也不要给用户一个看似聪明的错树。
     */
    async assignDomains(p: Proposal): Promise<'llm' | 'rule' | 'none'> {
      const data = useDataStore()
      const s = data.settings
      const strong = p.topics.filter((t) => !t.weak)
      const input = strong.map((t) => ({ name: t.name, weight: t.fileIds.length }))
      const names = input.map((t) => t.name)

      let result: DomainResult | null = null
      let source: 'llm' | 'rule' | 'none' = 'none'

      if (s.llmEnabled && input.length >= 4) {
        this.ai.current = '正在让本地模型判断这台电脑在管哪些方向的文件…'
        try {
          const r = await judgeDomainsWithLlm(input, {
            baseUrl: s.llmBaseUrl,
            model: s.llmModel,
            apiKey: s.llmApiKey,
          })
          if (isUsable(r, names)) {
            result = r
            source = 'llm'
          } else {
            this.ai.current = '模型没分出可用的大类，改用名字里的共同词粗略分组…'
          }
        } catch {
          this.ai.current = '模型没有响应，按名字里的共同词粗略分组…'
        }
      }

      if (!result) {
        const r = domainsFromSharedToken(input)
        if (r.domains.length > 0) {
          result = r
          source = 'rule'
        }
      }

      if (result) {
        for (const t of strong) {
          const d = result.assign.get(t.name)
          if (d) t.domain = d
        }
      }
      this.domainSource = source
      return source
    },

    /** 改名：只改展示名，别名不变，归属不变 */
    renameTopic(id: string, name: string) {
      if (!this.proposal) return
      const t = this.proposal.topics.find((x) => x.id === id)
      if (!t) return
      t.name = name.trim() || t.name
      this.refreshProposal()
    },

    /** 删掉的分类，文件回到未归类，不会丢 */
    removeTopic(id: string) {
      if (!this.proposal) return
      this.proposal = reassign(
        rawFiles,
        this.proposal.topics.filter((t) => t.id !== id)
      )
    },

    /** 改大类名：该域下所有事项一起改（目录名只有一个） */
    renameDomain(from: string, to: string) {
      const name = to.trim()
      if (!this.proposal || !name || name === from) return
      for (const t of this.proposal.topics) {
        if (t.domain === from) t.domain = name
      }
      this.proposal = { ...this.proposal }
    },

    /** 解散大类：事项回到第一层，文件和类目都不动 */
    dissolveDomain(name: string) {
      if (!this.proposal) return
      for (const t of this.proposal.topics) {
        if (t.domain === name) t.domain = undefined
      }
      this.proposal = { ...this.proposal }
    },

    /** 把某个事项挪到别的大类（domain 传 null 表示放回第一层） */
    moveTopicToDomain(id: string, domain: string | null) {
      if (!this.proposal) return
      const t = this.proposal.topics.find((x) => x.id === id)
      if (!t) return
      t.domain = domain?.trim() || undefined
      this.proposal = { ...this.proposal }
    },

    /** 批量删除：确认页上一次性清掉扫出来的垃圾类 */
    removeTopics(ids: string[]) {
      if (!this.proposal || ids.length === 0) return
      const drop = new Set(ids)
      this.proposal = reassign(
        rawFiles,
        this.proposal.topics.filter((t) => !drop.has(t.id))
      )
    },

    /**
     * 用户手动补一个分类。
     *
     * 系统漏掉的事，用户补个名字就能把文件捞回来——这比让用户去逐个文件指定省事得多。
     * 名字同时进文件名和正文匹配，所以像「XX 验收」这种只在正文出现的也能捞到。
     */
    addTopic(name: string, domain?: string) {
      const n = name.trim()
      if (!this.proposal || !n) return
      if (this.proposal.topics.some((t) => t.name === n)) return
      const id = `u${Date.now().toString(36)}`
      this.proposal = reassign(rawFiles, [
        ...this.proposal.topics,
        { id, name: n, aliases: [n], fileIds: [], samples: [], weak: false, domain: domain?.trim() || undefined },
      ])
    },

    /** 合并：把 from 的别名并入 to，文件随之并过去 */
    mergeTopics(fromId: string, toId: string) {
      if (!this.proposal || fromId === toId) return
      const from = this.proposal.topics.find((t) => t.id === fromId)
      const to = this.proposal.topics.find((t) => t.id === toId)
      if (!from || !to) return
      to.aliases = Array.from(new Set([...to.aliases, ...from.aliases]))
      this.proposal = reassign(
        rawFiles,
        this.proposal.topics.filter((t) => t.id !== fromId)
      )
    },

    /** 改完立刻重算归属（纯 CPU，毫秒级） */
    refreshProposal() {
      if (!this.proposal) return
      this.proposal = reassign(rawFiles, this.proposal.topics)
    },

    /** 用户点「确认」——此刻才真正落成分类目录 */
    confirmProposal() {
      if (!this.proposal) return
      const data = useDataStore()
      const applied = applyProposal(rawFiles, this.proposal)
      data.setResult(applied.files, applied.categories)
      const byKind: Record<string, number> = {}
      for (const f of applied.files) byKind[f.kind] = (byKind[f.kind] ?? 0) + 1
      void this.savePlan()
      this.summary = {
        categories: applied.categories,
        fileCount: applied.files.length,
        autoK: applied.categories.length,
        silhouette: 0,
        byKind,
      }
      this.next()
    },

    /**
     * 读上次的方案存档（程序启动时调一次）。
     * 读不到不算错——第一次用本来就没有存档。
     */
    async loadSavedPlan() {
      try {
        const s = await dbGet<SavedPlan>('savedPlan')
        this.savedPlan = s && Array.isArray(s.topics) && s.topics.length ? s : null
      } catch {
        this.savedPlan = null
      }
    },

    /**
     * 沿用存档：直接按老类目认领这一批文件，不跑候选抽取、不跑模型。
     *
     * 适用场景：
     *  · 上次整理完，这周又来了一批新文件 → 新文件自动落进老类目
     *  · 上次的方案你花时间调过，不想每次重调
     * 返回是否沿用成功；失败时调用方提示「存档没匹配到任何文件，请重新生成」。
     */
    useSavedPlan(): boolean {
      if (!this.savedPlan) return false
      const p = buildProposalFromSaved(rawFiles, this.savedPlan)
      if (!p || p.topics.length === 0) return false
      this.proposal = p
      this.whoExcluded = []
      this.ai = {
        ...emptyAi(),
        phase: 'done',
        done: rawFiles.length,
        total: rawFiles.length,
        current: `沿用上次方案：${p.topics.length} 个分类，${p.ungroupedIds.length} 份待指定`,
      }
      return true
    },

    /** 把当前提案存成下次可沿用的方案（确认时自动调） */
    async savePlan() {
      if (!this.proposal) return
      const plan = toSavedPlan(this.proposal, this.archivePath)
      this.savedPlan = plan
      try {
        await dbSet('savedPlan', plan)
      } catch {
        // 存不下不影响本轮整理，下次重新调一遍而已
      }
    },

    /** 丢掉存档（用户觉得这套类目不好用了） */
    async forgetSavedPlan() {
      this.savedPlan = null
      try {
        await dbSet('savedPlan', null)
      } catch {
        /* 忽略 */
      }
    },

    /** 中止本轮归纳（已判完的保留，剩余按规则结果收尾） */
    cancelInduction() {
      this.cancelled.cancelled = true
    },

    next() {
      if (this.step < 8) this.step++
    },
    back() {
      if (this.step > 1) this.step--
    },
    goto(step: number) {
      this.step = step
    },

    reset() {
      this.step = 1
      this.scanning = false
      this.inducing = false
      this.migrating = false
      this.summary = null
      this.proposal = null
      this.whoExcluded = []
      this.fileIndex = {}
      this.filePathIndex = {}
      this.fileMeta = {}
      this.dupIndex = {}
      this.error = ''
      this.ai = emptyAi()
      this.progress = { processed: 0, total: 0, currentPath: '', skipped: 0 }
      rawFiles = []
      rootHandleRef = null
    },

    /** 确保归档根存在（桌面版） */
    async ensureArchive() {
      if (isDesktop) await prepareArchiveRoot(this.archivePath)
    },
  },
})
