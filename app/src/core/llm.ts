import type { FileItem } from './types'
import { WORKSTREAM_LEXICON, DOCTYPE_LEXICON } from './classifier'
import { llmHttp } from './desktop'
import { buildWhoPrompt, parseWhoResult, type WhoKind } from './emergence/whoJudge'
import {
  buildDomainPrompt,
  parseDomainResult,
  type DomainResult,
  type DomainTopic,
} from './emergence/domain'
import type { Candidate } from './emergence/candidates'

/**
 * 本地大模型分类（Ollama 原生接口）
 *
 * 设计依据来自 `LLM评测报告.md` 的四条实测结论：
 *
 *  1. **结构化输出必须强约束**：早期给自由 JSON，模型会发明「技术方案」
 *     「智能工厂平台/钻采院项目」这类清单外答案。这里用 Ollama 的
 *     `format = JSON Schema` + enum 把可选值钉死，从解码层杜绝发明分类。
 *  2. **qwen3 系列必须 `think:false`**：否则每份文件多输出 190+ token 推理，
 *     单份耗时从 3s 涨到 26s。
 *  3. **模型最大的毛病是无正文也硬猜**（24 份样本 0 次说"不知道"）。
 *     所以 prompt 里把"没有证据就留空"写成硬指令，并给了明确的留空示例。
 *  4. **分类清单放在 system 消息里**：内容固定 → Ollama 可复用前缀缓存，
 *     后续文件只需 eval 文件本身那几十个 token。
 */

export const DEFAULT_BASE_URL = 'http://127.0.0.1:11434'
export const DEFAULT_MODEL = 'qwen2.5:7b'

export interface LlmConfig {
  baseUrl: string
  model: string
  apiKey: string
  /** 单份请求超时（毫秒） */
  timeoutMs?: number
}

export interface LlmVerdict {
  ws: string | null
  dt: string | null
  confidence: number
  /** 模型自述依据（可空） */
  why?: string
  latencyMs: number
}

/* ------------------------------------------------------------------ 健康检查 */

export interface OllamaModelInfo {
  name: string
  sizeGB: number
  /** 是否支持视觉（qwen3.5 系列） */
  vision: boolean
}

/** 列出本地已装模型；服务没起时返回 ok:false，由上层降级到规则引擎 */
export async function listModels(
  baseUrl = DEFAULT_BASE_URL,
  timeoutMs = 2500
): Promise<{ ok: boolean; models: OllamaModelInfo[]; error?: string }> {
  try {
    const json = (await llmHttp(
      'GET',
      `${baseUrl.replace(/\/$/, '')}/api/tags`,
      undefined,
      timeoutMs
    )) as { models?: Array<{ name: string; size?: number; capabilities?: string[] }> }
    const models: OllamaModelInfo[] = (json?.models ?? []).map((m) => ({
      name: m.name,
      sizeGB: Math.round(((m.size ?? 0) / 1e9) * 10) / 10,
      vision: (m.capabilities ?? []).includes('vision'),
    }))
    return { ok: true, models }
  } catch (e) {
    return {
      ok: false,
      models: [],
      error: e instanceof Error ? e.message : '无法连接本地模型服务',
    }
  }
}

/* ------------------------------------------------------------------ 提示词 */

const WORKSTREAM_NAMES = WORKSTREAM_LEXICON.map((d) => d.name)
const DOCTYPE_NAMES = DOCTYPE_LEXICON.map((d) => d.name)

/** 分类清单（固定内容，放在 system 里吃前缀缓存） */
function taxonomyBlock(): string {
  const ws = WORKSTREAM_LEXICON.map((d) => `- ${d.name}：${d.desc.slice(0, 42)}`).join('\n')
  const dt = DOCTYPE_LEXICON.map((d) => `- ${d.name}：${d.desc.slice(0, 42)}`).join('\n')
  return `【工作域 · 一级，只能选这些】\n${ws}\n\n【文档类型 · 二级，只能选这些】\n${dt}`
}

const SYSTEM_PROMPT = `你是一位技术管理者的文件归档助手。用户会把散落在电脑各处的文件交给你，你需要判断每份文件属于哪个工作域、是什么文档类型。

判断规则（必须遵守）：
1. 只能从下面的分类清单里选，绝对不能发明清单外的名称。
2. 判断「工作域」只看文件名里的项目名或事务专名（如「钻采院」「油化厂」「差旅费」「绩效考核」「智擎」）。正文里出现某个词，不代表这份文件属于那个项目。
3. 判断「文档类型」可以看正文，但不要因为正文提到"合同、成本、风险、流程"就判成对应类型——很多流程表会同时提到这些词。
4. **没有证据就必须留空**：如果正文是空的或抽不到，且文件名也没有任何专名（例如「final_v2.docx」「新建文档」「20260917-185927」「扫描件001」），就把 ws 和 dt 都返回空字符串。宁可说不知道，也绝不猜测。
5. **项目名不是归口**：如果文件名里出现的是明确的职能专名（如「安全检查报告」「费率清单」「外包管控」「绩效考核」「差旅报销」「服务器配置清单」），就按那个职能归口判工作域，不要因为正文反复提到某个项目名（如「智能工厂平台」）就把文件归到那个项目——项目名只是这份文件的背景。

只输出 JSON，不要任何解释文字。

${taxonomyBlock()}`

/** JSON Schema：把枚举钉死，从解码层杜绝发明分类 */
function responseSchema() {
  // 只输出 ws / dt 两个字段：实测每多一个字段就多 10 个 token，
  // 而 7B 模型在 4060 上只有 ~7 token/s —— 两个字段 3.8s，四个字段 6.5s。
  // 置信度交给融合层按证据来源给（模型自评的 conf 在评测里从不承认"不知道"，不可信）。
  return {
    type: 'object',
    properties: {
      ws: { type: 'string', enum: [...WORKSTREAM_NAMES, ''] },
      dt: { type: 'string', enum: [...DOCTYPE_NAMES, ''] },
    },
    required: ['ws', 'dt'],
  }
}

function userMessage(f: FileItem): string {
  const kb = Math.max(1, Math.round(f.size / 1024))
  const body = (f.textSummary || '').trim()
  const nodes = f.drawioNodes?.length ? f.drawioNodes.join('、') : ''
  const lines = [
    `文件名：${f.name}`,
    `类型：${f.kind} · ${kb}KB`,
    body
      ? `正文摘录：${body.slice(0, 700)}`
      : '正文摘录：（未抽取到正文，只有文件名可用）',
  ]
  if (nodes) lines.push(`图形内文字：${nodes.slice(0, 400)}`)
  return lines.join('\n')
}

/* ------------------------------------------------------------------ 请求 */

interface ChatOpts {
  /** 只要 1 个 token，用于预热加载模型 */
  warmup?: boolean
}

function needThinkOff(model: string): boolean {
  // qwen3 / qwen3.5 是思考型模型，不关掉会多输出一两百 token 推理
  return /qwen3/i.test(model)
}

async function ollamaChat(
  f: FileItem,
  cfg: LlmConfig,
  opts: ChatOpts = {}
): Promise<LlmVerdict> {
  const started = performance.now()
  const url = `${cfg.baseUrl.replace(/\/$/, '')}/api/chat`
  const body: Record<string, unknown> = {
    model: cfg.model || DEFAULT_MODEL,
    stream: false,
    keep_alive: '10m',
    messages: [
      { role: 'system', content: SYSTEM_PROMPT },
      { role: 'user', content: opts.warmup ? '预热' : userMessage(f) },
    ],
    options: {
      temperature: 0,
      top_p: 0.9,
      num_predict: opts.warmup ? 1 : 96,
      // 上下文压到 3072：正文只截 700 字，多余上下文只会拖慢 prompt 处理
      num_ctx: 3072,
      repeat_penalty: 1.05,
    },
  }
  if (!opts.warmup) body.format = responseSchema()
  if (needThinkOff(cfg.model)) body.think = false

  let json: { message?: { content?: string } }
  try {
    json = (await llmHttp(
      'POST',
      url,
      body,
      cfg.timeoutMs ?? 60000
    )) as { message?: { content?: string } }
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    throw new Error(/timeout|timed out|超时/i.test(msg) ? '模型响应超时' : `无法连接本地模型：${msg}`)
  }
  const latencyMs = Math.round(performance.now() - started)
  if (opts.warmup) return { ws: null, dt: null, confidence: 0, latencyMs }

  const raw: string = json?.message?.content ?? ''
  const parsed = parseVerdict(raw)
  return { ...parsed, latencyMs }
}

/** 容错解析：schema 已约束，但仍做一次白名单校验 */
function parseVerdict(raw: string): Omit<LlmVerdict, 'latencyMs'> {
  const empty = { ws: null, dt: null, confidence: 0, why: undefined }
  const m = raw.match(/\{[\s\S]*\}/)
  if (!m) return empty
  let obj: { ws?: string; dt?: string; conf?: number; confidence?: number; why?: string }
  try {
    obj = JSON.parse(m[0])
  } catch {
    return empty
  }
  const ws = typeof obj.ws === 'string' && WORKSTREAM_NAMES.includes(obj.ws) ? obj.ws : null
  const dt = typeof obj.dt === 'string' && DOCTYPE_NAMES.includes(obj.dt) ? obj.dt : null
  const confRaw = Number(obj.conf ?? obj.confidence ?? 0.7)
  const conf = Number.isFinite(confRaw) ? Math.max(0, Math.min(1, confRaw)) : 0
  // 什么都没判出来时置信度必须归零，避免"空答案高置信"
  const confidence = ws || dt ? conf : 0
  return { ws, dt, confidence, why: typeof obj.why === 'string' ? obj.why : undefined }
}

/* ------------------------------------------------------------------ 对外 */

/** 预热：把模型权重加载进显存，避免第一份文件卡 10 秒被当成卡死 */
export async function warmupModel(cfg: LlmConfig): Promise<boolean> {
  try {
    await ollamaChat({ id: 'warmup' } as FileItem, cfg, { warmup: true })
    return true
  } catch {
    return false
  }
}

/** 单份文件分类（连接异常重试一次；超时不重试） */
export async function classifyWithLlm(f: FileItem, cfg: LlmConfig): Promise<LlmVerdict> {
  try {
    return await ollamaChat(f, cfg)
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    // 超时重试只会更慢，直接抛出交给上层记为"未判出"
    if (/超时|timeout/i.test(msg)) throw e
    return await ollamaChat(f, cfg)
  }
}

/** 批量分类：小并发跑，边跑边回调（UI 实时展示） */
export async function classifyFiles(
  files: FileItem[],
  cfg: LlmConfig,
  opts: {
    concurrency?: number
    signal?: { cancelled: boolean }
    onEach?: (f: FileItem, v: LlmVerdict | null, err?: string) => void
  } = {}
): Promise<Map<string, LlmVerdict>> {
  const out = new Map<string, LlmVerdict>()
  const concurrency = Math.max(1, Math.min(8, opts.concurrency ?? 2))
  let cursor = 0
  let stopped = false

  const worker = async () => {
    while (!stopped) {
      if (opts.signal?.cancelled) {
        stopped = true
        return
      }
      const i = cursor++
      if (i >= files.length) return
      const f = files[i]
      try {
        const v = await classifyWithLlm(f, cfg)
        out.set(f.id, v)
        opts.onEach?.(f, v)
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        // 连接级错误（服务崩了）直接停，不要白白跑完剩下的
        if (/无法连接|Failed to fetch/i.test(msg)) stopped = true
        opts.onEach?.(f, null, msg)
      }
    }
  }

  await Promise.all(Array.from({ length: Math.min(concurrency, files.length) }, worker))
  return out
}

/**
 * 组织/人名判定：整个流程里唯一交给模型做的一步。
 *
 * 一次请求判完全部候选（实测 57 个候选 28-34 秒），不做并发——Ollama 默认串行推理，
 * 并发只会让同一批词排队，墙钟一样。
 *
 * 参数为什么是这几个：
 *  · num_ctx 4096 —— 实测 8192 会让 9b 被挤出显存跑到 CPU 上，整整 59 分钟没回；
 *    4096 + 约 1800 token 的 prompt 刚好全量进显存
 *  · num_predict 2048 —— 57 条 JSON 约 1300 token，留足余量但别浪费
 *  · 超时 90s —— 含模型加载（首次约 5-10s）+ 生成 30s
 */
export async function judgeWhoWithLlm(
  cands: Candidate[],
  cfg: LlmConfig
): Promise<Map<string, WhoKind>> {
  if (cands.length === 0) return new Map()
  const body: Record<string, unknown> = {
    model: cfg.model || DEFAULT_MODEL,
    stream: false,
    keep_alive: '10m',
    format: 'json',
    messages: [{ role: 'user', content: buildWhoPrompt(cands) }],
    options: { temperature: 0, num_ctx: 4096, num_predict: 2048 },
  }
  if (needThinkOff(cfg.model)) body.think = false

  const json = (await llmHttp(
    'POST',
    `${cfg.baseUrl.replace(/\/$/, '')}/api/chat`,
    body,
    Math.max(90_000, cfg.timeoutMs ?? 0)
  )) as { message?: { content?: string } }
  return parseWhoResult(json?.message?.content ?? '', cands)
}

/**
 * 分域：这台电脑主要在管哪些方向的文件（决定归档目录的第一层）。
 *
 * 为什么这件事敢交给模型：它不是「把 40 个词归并成组」那种它做不来的活，
 * 而是「看这批事项，判断这个人用电脑管什么」——一次归纳 + 一次归类。
 * 实测 qwen3.5:9b：40 个事项 → 4 个域（技术研发/项目建设/行政人事/财务差旅）、
 * 40/40 全归类、83 秒、433 token。域名字面的合理性和速度都能接受。
 *
 * 参数：
 *  · num_ctx 4096 —— 同 judgeWho：8192 会把 9b 挤出显存跑 CPU
 *  · num_predict 2048 —— 输出约 450 token，留余量
 *  · 超时 180s —— 实测 83s，加上首次加载模型
 */
export async function judgeDomainsWithLlm(
  topics: DomainTopic[],
  cfg: LlmConfig
): Promise<DomainResult> {
  if (topics.length === 0) return { domains: [], assign: new Map() }
  const body: Record<string, unknown> = {
    model: cfg.model || DEFAULT_MODEL,
    stream: false,
    keep_alive: '10m',
    format: 'json',
    messages: [{ role: 'user', content: buildDomainPrompt(topics) }],
    options: { temperature: 0, num_ctx: 4096, num_predict: 2048 },
  }
  if (needThinkOff(cfg.model)) body.think = false

  const json = (await llmHttp(
    'POST',
    `${cfg.baseUrl.replace(/\/$/, '')}/api/chat`,
    body,
    Math.max(180_000, cfg.timeoutMs ?? 0)
  )) as { message?: { content?: string } }
  return parseDomainResult(json?.message?.content ?? '', topics.map((t) => t.name))
}
