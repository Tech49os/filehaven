import type {
  AiSource,
  FileItem,
  HybridStats,
  HybridProgress,
  InductionResult,
} from './types'
import {
  induce,
  buildTaxonomy,
  suggestName,
  buildTags,
  type StagedFile,
} from './classifier'
import { classifyFiles, listModels, warmupModel, type LlmConfig, type LlmVerdict } from './llm'

/**
 * 规则 × 大模型 混合归纳
 *
 * 直接落评测报告里唯一站得住的方案：**分工 + 双路一致直通**。
 *
 *  - 工作域：模型整体略强（79.2% vs 70.8%），但两路**一致**时才敢自动归档
 *  - 文档类型：规则更强（87.5% vs 79.2%），一律规则优先，规则判不出才用模型补
 *  - 双路一致：评测里 12/12 全对，直接给 93% 自动归档——这是模型唯一真正
 *    创造价值的地方：把 74%~87% 的「半信半疑」抬成「可以不用看」
 *  - 两路不一致：进待确认，预填规则结论（依据是文件名，用户可自查），
 *    模型的不同意见写进理由
 *  - 规则判不出、模型判出来的「补盲」：预填模型答案，但同样进待确认
 *
 * 一句话：**模型负责把'对的'确认下来，不负责把'错的'改掉。**
 * 错标的代价（文件被静默塞进别人的项目）远高于多点一次确认。
 *
 * 之所以不做「全部文件都送模型」：452 个文件 × 2.4s ≈ 18 分钟，体验无法接受。
 * 默认 smart 策略只把规则没把握的那批送模型（通常是 20%~40%），几分钟内出结果；
 * 需要最高准确率时可在设置里切到 all。
 */

export interface HybridLlmOptions {
  enabled: boolean
  config: LlmConfig
  strategy: 'smart' | 'all'
  concurrency: number
  /** 0 = 不限 */
  maxFiles: number
}

export interface HybridOptions {
  threshold: number
  llm: HybridLlmOptions
  onProgress?: (p: HybridProgress) => void
  signal?: { cancelled: boolean }
}

export interface HybridResult {
  result: InductionResult
  files: FileItem[]
  stats: HybridStats
}

/** 规则在什么情况下值得让模型复核 */
function needsLlm(f: FileItem): boolean {
  // 第一条也是最重要的一条：**没有正文就不要送模型**。
  // 模型只能看到文件名 + 正文，没有正文时它掌握的信息和规则完全一样，
  // 却更容易瞎猜——评测里 24 份样本 0 次说"不知道"就是这个毛病。
  // 这类文件送进去既浪费时间又可能把正确答案改错。
  const hasBody = (f.textSummary?.trim().length ?? 0) > 30 || !!f.drawioNodes?.length
  if (!hasBody) return false

  // 源码文件不送：真实语料上实测，模型会把 components.js / tickets.html 这类
  // 前端源码当成文档读，判成「部门管理 · 单据表单」（看到 ticket 就想到工单）。
  // 这不是归档对象，判错了还要人去清理，直接交给规则结果（无工作域 → 待确认）。
  if (f.kind === 'code' || f.kind === 'other') return false

  // 规则没判出工作域 → 模型补盲价值最高
  if (!f.ws) return true
  // 置信度不够硬（兜底 0.74 / 正文推断 0.78 / 单一弱证据 0.86）都值得复核。
  // 这批文件正是「双路一致」最值钱的地方：一致后可把 0.74 抬到 0.93，
  // 让本来要人工确认的文件直接自动归档。
  return f.confidence < 0.9
}

/** 排序：先送最需要模型的那批（无工作域 → 置信度低 → 其他） */
function priorityOf(f: FileItem): number {
  if (!f.ws) return 0
  return 1 + f.confidence
}

interface Fused {
  ws: string | null
  dt: string | null
  confidence: number
  source: AiSource
  reason: string
}

/** 单份文件的融合判定（纯函数，便于测试） */
export function fuseVerdict(
  rule:
    | { ws?: string | null; dt?: string | null; confidence?: number; reason?: string }
    | undefined,
  llm: LlmVerdict | null,
  threshold: number
): Fused {
  const rWs = rule?.ws ?? null
  const rDt = rule?.dt ?? null
  const rConf = rule?.confidence ?? 0.5

  if (!llm) {
    return {
      ws: rWs,
      dt: rDt,
      confidence: rConf,
      source: rWs ? 'rule' : 'none',
      reason: rule?.reason ?? '没有足够特征，无法判断',
    }
  }

  const lWs = llm.ws
  const lDt = llm.dt
  let ws: string | null = rWs
  let confidence = rConf
  let source: AiSource = rWs ? 'rule' : 'none'
  let reason = rule?.reason ?? ''

  if (rWs && lWs && rWs === lWs) {
    // 双路一致 → 评测里 100% 正确，直接给高置信
    ws = rWs
    confidence = Math.min(0.98, Math.max(rConf, 0.9) + 0.03)
    source = 'agree'
    reason = `规则与模型独立判断一致，双方都指向「${rWs}」`
  } else if (rWs && lWs) {
    // 分歧：**一律进待确认，绝不静默改判**，预填保留规则结论。
    //
    // 为什么不预填模型：真实语料上模型有个稳定的偏差——正文里只要反复出现
    // 某个主干项目名（如「智能工厂平台」），它就把「安全检查报告」「工作量
    // 成本」「服务器配置清单」统统归到那个项目，职能归口（安全合规/商务采购）
    // 被项目名吃掉。演示集里 4 处分歧、真实语料 4 处分歧，都是这个形态。
    // 规则的依据是文件名里的专名，用户扫一眼文件名就能自己核对；
    // 所以预填规则、把模型的不同意见写进理由，让用户点一下裁决。
    ws = rWs
    confidence = Math.min(0.66, threshold - 0.04)
    source = 'conflict'
    reason = `规则按文件名判为「${rWs}」，模型读正文认为是「${lWs}」，两者不一致，请你确认`
  } else if (lWs) {
    // 模型补盲：规则完全判不出，模型读正文后给出了结论。
    // 预填模型答案，但不让它自动归档——没有第二路交叉验证，错标的代价是
    // 文件被静默塞进别人的项目里，比多一次确认贵得多。
    ws = lWs
    // 模型自评置信度只作微调，不直接采信（它普遍偏自信）
    confidence = Math.min(0.8, llm.confidence >= 0.5 ? 0.68 + 0.12 * llm.confidence : 0.62)
    source = 'llm'
    const why = llm.why?.trim()
    reason = `规则没有可用证据，模型读正文后判为「${lWs}」${why ? `（依据：${why.slice(0, 40)}）` : ''}`
    if (!llm.dt) confidence = Math.max(0.6, confidence - 0.05)
  } else if (rWs) {
    ws = rWs
    source = 'rule'
    reason = rule?.reason ?? `规则判定为「${rWs}」`
  } else {
    // 双方都说不知道 —— 这是诚实的结果，交给用户
    ws = null
    confidence = 0.45
    source = 'none'
    reason = '正文与文件名都没有可用信息，模型也没有猜，请手动归类'
  }

  // 类型：规则优先（87.5% vs 79.2%），规则判不出才用模型
  const dt = rDt ?? lDt ?? null
  // 类型判断不准只微降置信度（文件毕竟还在正确项目里）
  if (!dt) confidence = Math.max(0.4, confidence - 0.04)

  return { ws, dt, confidence: Math.max(0.02, Math.min(0.98, confidence)), source, reason }
}

/**
 * 混合归纳主流程
 *
 * 顺序：规则初判 → 挑出需要复核的文件 → 预热模型 → 并发跑模型 → 逐份融合 → 建树
 * 全程通过 onProgress 把每一份的结果推给 UI（边判边看，而不是干等进度条）。
 */
export async function induceHybrid(
  files: FileItem[],
  opts: HybridOptions
): Promise<HybridResult> {
  const started = Date.now()
  const { threshold, llm } = opts
  const stats: HybridStats = {
    total: files.length,
    llmUsed: false,
    llmCandidates: 0,
    llmDone: 0,
    agree: 0,
    conflict: 0,
    filled: 0,
    ruleOnly: 0,
    unknown: 0,
    elapsedMs: 0,
    model: llm.config.model,
    cancelled: false,
  }

  // 1) 规则初判（毫秒级）
  opts.onProgress?.({ phase: 'rules', done: 0, total: files.length, current: '规则引擎快速初判…' })
  const base = induce(files, { threshold })
  const ruleMap = new Map(base.files.map((f) => [f.id, f]))

  // 2) 挑出需要模型复核的文件
  let candidates: FileItem[] = []
  let fallbackReason: string | undefined

  if (llm.enabled && files.length > 0) {
    candidates = files
      .filter((f) => {
        const r = ruleMap.get(f.id)
        if (!r) return false
        if (llm.strategy === 'smart') return needsLlm(r)
        // 全部交叉验证：也跳过没正文的和源码（模型看不到更多信息，只会增加误判）
        if (r.kind === 'code' || r.kind === 'other') return false
        return (r.textSummary?.trim().length ?? 0) > 30 || !!r.drawioNodes?.length
      })
      // 最需要模型排前面：万一用户中途点"先看已判完的"，价值最高的已经跑完了
      .sort((a, b) => priorityOf(a) - priorityOf(b))
    if (llm.maxFiles > 0 && candidates.length > llm.maxFiles) {
      candidates = candidates.slice(0, llm.maxFiles)
    }
    if (candidates.length === 0) {
      fallbackReason = '规则已高置信判完全部文件，本次未调用模型'
    } else {
      // 探测服务：没起就体面降级，不要让用户看到满屏报错
      const probe = await listModels(llm.config.baseUrl)
      if (!probe.ok) {
        fallbackReason = `未连接本地模型服务（${probe.error}），已退回规则引擎`
        stats.llmCandidates = 0
        candidates = []
      } else if (!probe.models.some((m) => m.name === llm.config.model)) {
        fallbackReason = `本地没有模型「${llm.config.model}」，已退回规则引擎`
        stats.llmCandidates = 0
        candidates = []
      } else {
        stats.llmUsed = true
        stats.llmCandidates = candidates.length
        opts.onProgress?.({
          phase: 'warmup',
          done: 0,
          total: candidates.length,
          current: `正在加载模型 ${llm.config.model}…`,
        })
        await warmupModel(llm.config)
      }
    }
  }

  // 3) 模型并发复核
  const verdicts = new Map<string, LlmVerdict>()
  if (candidates.length > 0) {
    let done = 0
    await classifyFiles(candidates, llm.config, {
      concurrency: llm.concurrency,
      signal: opts.signal,
      onEach: (f, v) => {
        done++
        if (v) verdicts.set(f.id, v)
        // 每份都立刻融合一次，UI 才能边跑边显示
        const fused = fuseVerdict(ruleMap.get(f.id), v, threshold)
        opts.onProgress?.({
          phase: 'llm',
          done,
          total: candidates.length,
          current: f.name,
          event: {
            id: f.id,
            name: f.name,
            ws: fused.ws,
            dt: fused.dt,
            confidence: fused.confidence,
            source: fused.source,
            reason: fused.reason,
          },
        })
      },
    })
    stats.llmDone = verdicts.size
    if (opts.signal?.cancelled) stats.cancelled = true
  }

  // 4) 逐份融合 + 建树
  opts.onProgress?.({ phase: 'tree', done: 0, total: files.length, current: '正在生成分类目录…' })

  const staged: StagedFile[] = files.map((f) => {
    const rule = ruleMap.get(f.id)
    const fused = fuseVerdict(rule, verdicts.get(f.id) ?? null, threshold)
    const needReview =
      fused.confidence < threshold || !fused.ws || fused.source === 'conflict'

    if (fused.source === 'agree') stats.agree++
    else if (fused.source === 'conflict') stats.conflict++
    else if (fused.source === 'llm') stats.filled++
    else if (fused.source === 'rule') stats.ruleOnly++
    if (!fused.ws) stats.unknown++

    const llmV = verdicts.get(f.id)
    return {
      ...f,
      confidence: fused.confidence,
      needReview,
      status: needReview ? ('pending' as const) : ('classified' as const),
      reason: fused.reason,
      aiSource: fused.source,
      ws: fused.ws,
      dt: fused.dt,
      llmWs: llmV?.ws ?? null,
      llmDt: llmV?.dt ?? null,
      suggestedName: suggestName(f, fused.dt, fused.ws),
      tags: buildTags(fused.ws, fused.dt, f),
      _ws: fused.ws,
      _dt: fused.dt,
    }
  })

  // 混合模式下允许多份小类：模型判出的单份文件通常确实独立
  const { result, files: finalFiles } = buildTaxonomy(staged, threshold, 1)

  stats.elapsedMs = Date.now() - started
  stats.fallbackReason = fallbackReason
  opts.onProgress?.({ phase: 'done', done: files.length, total: files.length, current: '' })

  return { result, files: finalFiles, stats }
}
