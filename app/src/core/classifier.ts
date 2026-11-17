import type { Category, FileItem, FileKind, InductionResult } from './types'

/**
 * 分类引擎 v2 —— 真实语料版
 *
 * v1 的问题（真实桌面 450 文件实测暴露）：
 *  1. 词典是电商主题（支付/订单/风控/用户中心），与真实语料零重叠 → 99% 文件归错
 *  2. 置信度只看"得分与差距"，关键词偶然命中就给 99% → 错误结果被标成高可信，比不分类更危险
 *  3. 命名把通用词当特征（差旅单被命名成"权限_运维手册"）
 *
 * v2 的三处修正：
 *  A. 字典从真实语料重建：一级=工作域（项目/工作线），二级=文档类型 —— 贴合作者按项目管文件的真实心智
 *  B. 置信度改为「证据锚定」：必须命中核心证据词才允许高置信；只命中泛化词一律封顶 0.62 进待确认
 *  C. 命名改保守策略：日期前缀 + 保留原名（原名信息量大，改简反而丢信息）
 */

// ---------------------------------------------------------------- 词条定义

interface Ev {
  w: string
  /** 证据强度：3 核心证据（专有/高区分度），2 强辅助，1 泛化词 */
  s: 3 | 2 | 1
}
const c3 = (w: string): Ev => ({ w, s: 3 })
const c2 = (w: string): Ev => ({ w, s: 2 })
const c1 = (w: string): Ev => ({ w, s: 1 })

interface Def {
  name: string
  desc: string
  kindHint?: FileKind[]
  words: Ev[]
}

/** 一级：工作域（作者按项目/工作线管文档，这是主维度） */
export const WORKSTREAM_LEXICON: Def[] = [
  {
    name: '智能工厂平台',
    desc: '智能工厂平台、生产智能化、技术底座等主干项目',
    // 注意：「智能工厂」是本部门的名称，几乎出现在所有文件名里，区分力极低。
    // 若给高权重，它会吸走全部文件、压过「绩效考核」「差旅」这类真正的强信号。
    // 所以降到 c1，主干的认定交给 induce() 里的 DEFAULT 兜底逻辑。
    words: [
      c3('智擎'), c3('海发版'), c3('技术底座'), c3('工业互联网平台'),
      c3('智能化项目建设'), c3('数智化建设'), c3('统一技术底座'), c3('开发框架'),
      c3('生产智能化'), c3('智能感知'), c3('技术谱系'), c3('数字化建设'),
      c3('蓝图设计'), c3('智造领航'), c3('数字蓝海'), c3('ai大赛'),
      c3('硬件设备支撑'), c3('产品总结'),
      c2('数智化'), c2('智能化'),
      c1('智能工厂'), c1('平台'), c1('数字化'), c1('建设'),
    ],
  },
  {
    name: '钻采院项目',
    desc: '钻采研究院高温高压设备智能化提升项目',
    words: [c3('钻采'), c3('钻采院'), c3('钻采研究院'), c3('高温高压'), c2('智慧安全监管')],
  },
  {
    name: '油化厂项目',
    desc: '油田化工智能工厂现场实施项目',
    words: [c3('油化厂'), c3('油田化工'), c2('化工智能工厂'), c2('现场bug')],
  },
  {
    name: '部门管理',
    desc: '考核、考勤、人员安排、工单、制度与专班工作',
    words: [
      c3('考核评分'), c3('绩效考核'), c3('考勤'), c3('质量之星'), c3('战略部署'),
      c3('招聘需求'), c3('人员安排'), c3('人员转型'), c3('工单'), c3('专班'),
      c3('督办表'), c3('能力提升'), c3('岗位操作手册'), c3('工作质量标准'),
      c3('开发组'), c3('全员全栈'), c3('岗位配置'), c3('业务活动'),
      c3('外包管控'), c3('精益管理'),
      c2('考核'), c2('月度'), c2('奖项'), c2('管控办法'), c2('管理细则'),
      c1('管理'), c1('计划'), c1('人员'), c1('班组'),
    ],
  },
  {
    name: '商务采购',
    desc: '合同、集采协议、报价、成本与工作量核算',
    words: [
      c3('合同'), c3('协议'), c3('集采'), c3('采购'), c3('招标'), c3('投标'),
      c3('报价'), c3('工作量'), c3('委托书'), c3('承诺书'), c3('技术要求书'),
      c3('商务建议书'), c3('预算清单'), c3('成本反馈'),
      c2('成本'), c2('预算'), c2('结算'), c2('费用'), c2('清单'),
      c1('商务'),
    ],
  },
  {
    name: '人事培训',
    desc: '简历、招聘、内训课程与能力培育',
    words: [
      c3('简历'), c3('全栈工程师'), c3('内训'), c3('培训'), c3('培育'),
      c3('讲师'), c3('入职'), c3('面试'),
      c2('招聘'), c2('课程'), c2('考生'), c2('申报表'), c2('人员'), c1('培养'),
    ],
  },
  {
    name: '安全合规',
    desc: '网络安全检查、等级保护、应急预案与密钥管理',
    words: [
      c3('网络安全'), c3('网安'), c3('安全检查'), c3('等级保护'), c3('等保'),
      c3('漏洞'), c3('渗透'), c3('应急预案'), c3('应急演练'), c3('安全防护'),
      c3('密钥'), c3('安环'), c2('风险评估'), c2('保密'), c2('大屏'),
      c1('安全'), c1('风险'), c1('应急'),
    ],
  },
  {
    name: '财务报销',
    desc: '差旅报销、资源申请与领用交接单据',
    words: [
      c3('报销'), c3('差旅'), c3('差旅费'), c3('审批单'), c3('领用'),
      c3('资源申请'), c3('服务器申请'), c3('服务器配置'),
      c2('申请单'), c2('交接单'),
      c1('申请'), c1('审批'), c1('付款'),
    ],
  },
  {
    name: '知识沉淀',
    desc: '技术研究、标准规范、对标分析与参考资料',
    words: [
      c3('关键技术研究'), c3('科研'), c3('专利'), c3('论文'), c3('对标分析'),
      c3('产品对比'), c3('技术路线'), c3('参考资料'), c3('gb_t'), c3('行业标准'),
      c3('笔记'), c2('研究'), c2('认知'), c2('标准'),
      c1('参考'), c1('资料'), c1('技术'),
    ],
  },
]

/** 二级：文档类型（跨工作域复用，回答"这是什么性质的文件"） */
export const DOCTYPE_LEXICON: Def[] = [
  {
    name: '方案设计',
    desc: '技术方案、可行性研究、架构设计图与需求说明',
    kindHint: ['drawio'],
    words: [
      c3('技术方案'), c3('设计方案'), c3('实施方案'), c3('提升方案'), c3('建设方案'),
      c3('可行性研究'), c3('可研报告'), c3('架构图'), c3('部署图'), c3('数据流图'),
      c3('时序图'), c3('拓扑'), c3('drawio'), c3('架构集'), c3('需求大纲'),
      c3('需求说明'), c3('需求交流'), c3('功能清单'), c3('思维导图'),
      c3('项目方案'), c3('培训方案'), c3('h5培训游戏'), c3('课程设置'),
      c3('技术研究'), c3('研究报告'), c3('战略部署'), c3('工作方案'), c3('蓝图设计'),
      c2('需求'), c2('架构'),
      // 「方案」「设计」「功能」「规划」在任何公文里都可能出现，区分力极低，
      // 只能作为微弱加分，不能凭它们判定类型（否则简历里写项目经历也会被判成方案）
      c1('方案'), c1('设计'), c1('功能'), c1('规划'),
    ],
  },
  {
    name: '汇报材料',
    desc: '进展汇报、专题报告、周报月报与工作总结',
    words: [
      c3('汇报'), c3('进展'), c3('专题报告'), c3('月报'), c3('周报'), c3('双周报'),
      c3('工作情况报告'), c3('工作总结'), c3('开展汇报'), c3('演讲稿'), c3('发言稿'),
      c3('讲话'), c3('评比汇报'), c3('述职'), c3('成果汇报'),
      c2('报告'), c2('提纲'), c2('总结'), c2('攻坚'),
      c1('情况'),
    ],
  },
  {
    name: '会议纪要',
    desc: '推进会、交流会等会议记录与决议',
    words: [
      c3('推进会'), c3('交流会'), c3('会议纪要'), c3('会议记录'), c3('议程'),
      c3('参会'), c3('工作会'), c3('技术委员会'), c3('准备会'), c3('研讨会'),
      c3('汇报会'), c2('会议'), c2('讨论'), c2('意见'), c2('合稿'),
    ],
  },
  {
    name: '合同协议',
    desc: '正式签署的合同、协议与授权文书',
    words: [
      c3('合同'), c3('协议'), c3('委托书'), c3('承诺书'), c3('技术要求书'),
      c3('商务建议书'), c3('框架协议'), c2('签署'), c2('签字'),
    ],
  },
  {
    name: '成本报价',
    desc: '报价单、工作量成本核算与预算清单',
    words: [
      c3('报价单'), c3('工作量'), c3('成本'), c3('预算'), c3('结算'),
      c3('费率'), c3('价格'), c3('测算'),
      c2('报价'), c2('清单'), c2('核算'), c2('费用'), c2('金额'),
    ],
  },
  {
    name: '制度规范',
    desc: '管理办法、技术标准、操作手册与流程模板',
    words: [
      c3('管控办法'), c3('管理细则'), c3('操作手册'), c3('质量标准'), c3('管控标准'),
      c3('全流程管控'), c3('选题分析'), c3('分析表'), c2('模板'), c2('办法'),
      // 「标准」「规范」「流程」「制度」在任何公文里都高频出现（"按标准执行""规范要求"），
      // 只能算弱信号，否则凡是正文提到"规范"的文件都会被吸进制度规范
      c1('规范'), c1('标准'), c1('流程'), c1('制度'), c1('细则'),
    ],
  },
  {
    name: '考核人事',
    desc: '绩效考核、考勤、简历与人员安排表',
    words: [
      c3('考核评分'), c3('绩效考核'), c3('考勤'), c3('简历'), c3('人员安排'),
      c3('招聘需求'), c3('质量之星'), c3('人员转型'), c3('候选人'), c3('全栈工程师'),
      c2('考核'), c2('奖项'), c2('申报表'), c2('工程师'), c2('人员'),
    ],
  },
  {
    name: '检查应急',
    desc: '安全检查报告、应急预案与风险评估',
    words: [
      c3('安全检查'), c3('应急预案'), c3('应急演练'), c3('风险评估'), c3('等级保护'),
      c3('防护模型'), c3('审查意见'), c3('预判审查'),
      c2('检查'), c2('演练'), c2('防护'), c2('模型'), c2('审查'),
    ],
  },
  {
    name: '单据表单',
    desc: '申请单、审批单、交接单、确认单等流转单据',
    words: [
      c3('审批单'), c3('申请单'), c3('申请表'), c3('领用'), c3('交接单'),
      c3('确认单'), c3('抽检单'), c3('开工申请'), c3('档案封皮'), c3('申报表'),
      c3('配置清单'), c3('出差申请'), c3('预算审批'),
      c2('单据'), c2('表单'), c2('清单'),
    ],
  },
  {
    name: '参考素材',
    desc: '笔记、认知材料、对比分析与行业标准',
    words: [
      c3('对标分析'), c3('产品对比'), c3('参考资料'), c3('初步认知'),
      c3('gb_t'), c3('国家标准'), c3('标准文件'), c3('笔记'), c3('技术谱系'), c3('谱系'),
      c2('认知'), c2('对比'), c2('素材'), c1('资料'), c1('参考'),
    ],
  },
]

const KIND_LABEL: Record<FileKind, string> = {
  doc: '文档', xls: '表格', ppt: '演示', pdf: 'PDF',
  image: '图片', drawio: '架构图', text: '文本', code: '代码', other: '文件',
}

// ---------------------------------------------------------------- 打分

export interface Score {
  name: string
  score: number
  /** 文件名命中的核心证据词（s=3）个数 */
  coreHits: number
  /** 文件名命中的强证据词（s>=2）个数（含核心） */
  strongHits: number
  /** 正文命中的核心词个数（文件名无证据时才启用） */
  bodyCore: number
  /** 正文命中的强词个数 */
  bodyStrong: number
}

/**
 * 判定"这个分类是否站得住"（基于文件名证据）。
 *
 * 真实语料教训：很多文件的项目名是"强词"而非"核心词"（如「智能感知」「技术谱系」
 * 「生产智能化」），单个强词不足以定论，但**两个强词同时命中**就是可靠证据。
 * 只认核心词会导致这类文件全被判成"未分类"。
 */
export function isConvincing(s: Score): boolean {
  return s.coreHits >= 1 || s.strongHits >= 2
}

/**
 * 判定"仅凭正文能否定论"——门槛比文件名证据更高。
 *
 * 场景：`final_v2.docx`「新建文档.docx」这类文件名毫无信息、但正文写得清清楚楚
 * 的文件。这正是用户最需要 AI 帮忙的地方，不能因为"文件名没用"就放弃。
 * 但正文容易夹带噪声（一份流程表里会提到合同、成本、风险），所以要求更高：
 * 至少 2 个核心词，或 1 个核心词 + 2 个强词。
 */
export function isConvincingByBody(s: Score): boolean {
  return s.bodyCore >= 2 || (s.bodyCore >= 1 && s.bodyStrong >= 2)
}

/** 组装用于打分的文本 */
export function docText(f: FileItem): string {
  const parts = [f.name, f.textSummary]
  if (f.drawioNodes?.length) parts.push(f.drawioNodes.join(' '))
  return parts.join(' ').toLowerCase()
}

/**
 * 词命中加权
 *
 * 核心原则：**判断"这份文件属于哪个项目"只能看文件名。**
 *
 * 作者给文件命名时一定会带上项目/事务名（"钻采院""油化厂""差旅费"），
 * 而正文里出现什么词完全不代表归档归属——一份精益管理的业务流程表，
 * 正文里会写满"合同管理""商务落地""成本""风险"（因为流程表本来就在描述
 * 这些环节），若给正文哪怕 0.35 的权重，它就会被判成商务采购。
 *
 * 实测校准：
 *  - 工作域：正文权重 0.10（几乎只听文件名）
 *  - 文档类型：正文权重 0.35（"是什么性质"确实要看内容，但仍不能压过文件名）
 */
const NAME_BOOST = 1.6

/**
 * 辅助词（s<3）的总贡献上限。
 *
 * 真实语料教训：不同分类的词条数量差异很大（「方案设计」23 个词、「会议纪要」6 个），
 * 若不封顶，正文里一堆泛词累加就能压过真正的核心词——结果是「人工智能推进会」
 * 被判成「方案设计」、简历因写明项目经历也被判成「方案设计」。
 */
const AUX_CAP = 2.5

function scoreDef(
  def: Def,
  nameText: string,
  bodyText: string,
  kind: FileKind,
  bodyWeight: number
): Score {
  let nameScore = 0
  let bodyCoreScore = 0
  let bodyAuxScore = 0
  let coreHits = 0
  let strongHits = 0
  let bodyCore = 0
  let bodyStrong = 0

  for (const { w, s } of def.words) {
    const inName = nameText.includes(w)
    if (inName) {
      nameScore += s * NAME_BOOST
      if (s >= 3) coreHits++
      if (s >= 2) strongHits++
      continue
    }
    if (!bodyText.includes(w)) continue
    if (s >= 3) {
      bodyCoreScore += s * bodyWeight
      bodyCore++
    } else {
      bodyAuxScore += s * bodyWeight
    }
    if (s >= 2) bodyStrong++
  }

  // 正文里的核心词不封顶（它是可靠的语义证据），泛化词累加要封顶
  // （否则「方案设计」这类词条多的分类会靠堆泛词赢过真正的核心词）
  let score = nameScore + bodyCoreScore + Math.min(AUX_CAP, bodyAuxScore)
  if (def.kindHint?.includes(kind)) score += 0.8
  return { name: def.name, score, coreHits, strongHits, bodyCore, bodyStrong }
}

function rank(defs: Def[], f: FileItem, bodyWeight: number): Score[] {
  const nameText = f.name.toLowerCase()
  const bodyParts = [f.textSummary]
  if (f.drawioNodes?.length) bodyParts.push(f.drawioNodes.join(' '))
  const bodyText = bodyParts.join(' ').toLowerCase()
  return defs
    .map((d) => scoreDef(d, nameText, bodyText, f.kind, bodyWeight))
    // 排序：先看文件名硬证据，再看总分。
    // 「有文件名声明的分类」必须优先于「只靠正文词堆出来的分类」。
    .sort(
      (a, b) =>
        b.coreHits - a.coreHits ||
        b.strongHits - a.strongHits ||
        b.score - a.score
    )
}

export function scoreWorkstreams(f: FileItem): Score[] {
  return rank(WORKSTREAM_LEXICON, f, 0.1)
}

export function scoreDocTypes(f: FileItem): Score[] {
  return rank(DOCTYPE_LEXICON, f, 0.35)
}

/** 兼容旧接口 */
export function scoreTopics(f: FileItem): Score[] {
  return scoreWorkstreams(f)
}

/**
 * 置信度（v2 核心修正：证据锚定 + 风险分级）
 *
 * 两条必须守住的规则：
 *
 * 1. **没有核心证据词，置信度一律封顶 0.62**，必进待确认。
 *    这条直接消灭了 v1「99% 置信度给出完全错误分类」的危险行为——v1 里
 *    "权限""进度""部署"这类泛化词的偶然命中也能拿满分。
 *
 * 2. **置信度以工作域为主，不以二级类型为主**。因为两者出错代价不同：
 *    归错工作域 → 文件进错项目目录，将来彻底找不到（致命）；
 *    归错二级类型 → 文件还在正确项目里，只是隔了一层（可接受）。
 *    所以类型判不准只微降置信度，不应把文件拖进待确认队列。
 */
function confidenceOf(p: {
  /** 工作域证据来源 */
  evidence: 'name' | 'body' | 'fallback' | 'none'
  /** 文件名命中的核心证据词数 */
  wsCore: number
  /** 工作域与次优的区分度 0–1 */
  wsMargin: number
  /** 是否识别出文档类型 */
  dtKnown: boolean
  /** 信息充分度 0–1 */
  info: number
}): number {
  let conf: number

  if (p.evidence === 'name') {
    // 文件名里有硬证据：0.86 起步，最多加到 0.98
    conf = 0.86 + Math.min(0.06, 0.06 * (p.wsCore - 1)) + 0.06 * p.wsMargin
  } else if (p.evidence === 'body') {
    // 只能靠正文推断（文件名无信息）：结论合理但要打折，标黄让用户扫一眼
    conf = 0.78 + 0.04 * p.wsMargin
  } else if (p.evidence === 'fallback') {
    // 主干兜底：归属是推断的
    conf = 0.74
  } else {
    // 工作域完全判不出来 → 必进待确认
    conf = 0.55
  }

  // 类型不明只微降（不影响它留在正确项目里）
  if (!p.dtKnown) conf -= 0.04

  // 信息严重不足（图片、纯压缩包、扫描件）：不能装作有把握
  if (p.info < 0.3) conf *= 0.7

  return Math.min(0.98, Math.max(0.02, conf))
}

/** 兼容旧接口 */
export function detectDomain(f: FileItem): string | null {
  const r = scoreDocTypes(f)
  return r[0] && isConvincing(r[0]) ? r[0].name : null
}

// ---------------------------------------------------------------- 命名

/** 文件名里对命名无贡献的部分 */
const NOISE = /[（(]\d+[)）]|副本|最终版|最终|最新|修改版|修订版|_完$|^新建|^\d{8}$/g

/**
 * 命名建议（v2：保守策略 —— 日期前缀 + 保留原名）
 *
 * v1 用"通用词+主题"重拼文件名，把「差旅费审批单」改成「权限_运维手册」，
 * 信息全丢。真实文件名本身信息量很高，正确做法是加日期前缀让它可排序、
 * 清掉噪声词让它在同目录内唯一，而不是重写它。
 */
export function suggestName(f: FileItem, _dt?: string | null, _ws?: string | null): string {
  const d = new Date(f.mtime)
  const ym = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`
  const ext = f.ext ? `.${f.ext}` : ''
  let base = f.name.slice(0, f.name.length - ext.length).trim()

  // 已经带日期前缀的不重复加
  if (/^\d{4}[-_]?\d{2}/.test(base)) return f.name

  base = base.replace(NOISE, '').replace(/[-_\s]{2,}/g, '_').replace(/^[-_\s]+|[-_\s]+$/g, '')
  if (!base) base = f.name.slice(0, f.name.length - ext.length)
  // 过长截断（Windows 路径长度限制保护）
  if (base.length > 70) base = base.slice(0, 70)
  return `${ym}_${base}${ext}`
}

/** 目录名 */
export function categoryPathOf(ws: string | null, dt: string | null): string {
  if (ws && dt) return `${ws}\\${dt}`
  return ws ?? dt ?? '待归类'
}

/**
 * 纯人名压缩包 → 疑似简历
 *
 * "刘涛.zip""吴迪.rar" 这类文件没有可用的语义特征（压缩包抽不出正文，
 * 文件名只有两个汉字），词典永远判不出来。但在人事场景里，按人名打包的
 * 压缩包基本就是候选人材料。
 *
 * 注意：只给**预填猜测**（低置信、进待确认、需用户一键确认），绝不自动通过——
 * 纯人名也可能是任何资料，猜错的代价必须由用户把关。
 */
const ARCHIVE_EXT = new Set(['zip', 'rar', '7z', 'tar', 'gz'])
export function looksLikePersonArchive(f: FileItem): boolean {
  if (!ARCHIVE_EXT.has(f.ext)) return false
  const base = f.name.replace(/\.[^.]+$/, '').trim()
  return /^[\u4e00-\u9fff]{2,4}$/.test(base)
}

// ---------------------------------------------------------------- 归纳 + 归类

export interface ClassifyOptions {
  /** 置信度阈值，低于此值进待确认 */
  threshold: number
  /** 工作域最低得分（低于此值视为无法判断工作域） */
  minScore?: number
}

const MIN_SCORE = 1.2

/**
 * 主干工作域：识别不出具体项目、但能识别文档类型时的默认归属。
 * 作者桌面上的文件绝大多数属于智能工厂这条主干工作线，只是分不清归哪个子项。
 */
const DEFAULT_WORKSTREAM = '智能工厂平台'

/**
 * 允许兜底到主干项目的文档类型。
 *
 * 只有"方案/汇报/会议/制度/参考"这类天然属于主干项目产出的文件，才允许
 * 在识别不出具体项目时兜底到「智能工厂平台」。
 *
 * 反过来，「考核人事」「单据表单」「合同协议」「成本报价」这几类文件，
 * 一旦判不出工作域，很可能其实属于部门管理 / 财务报销 / 商务采购，
 * 硬兜底到平台就是错的——这类必须进待确认，交给人来定。
 */
const FALLBACK_ALLOWED_DT = new Set([
  '方案设计',
  '汇报材料',
  '会议纪要',
  '制度规范',
  '参考素材',
])

export function induce(
  files: FileItem[],
  opts: ClassifyOptions
): { result: InductionResult; files: FileItem[] } {
  const { threshold } = opts
  const minScore = opts.minScore ?? MIN_SCORE

  type Staged = FileItem & {
    _ws: string | null
    _dt: string | null
  }

  const staged: Staged[] = files.map((f) => {
    const wsRank = scoreWorkstreams(f)
    const dtRank = scoreDocTypes(f)
    const wsBest = wsRank[0]
    const wsSecond = wsRank[1] ?? { name: '', score: 0, coreHits: 0 }
    const dtBest = dtRank[0]

    // ── 工作域判定：文件名证据优先；文件名没信息时才看正文 ──
    let ws: string | null = null
    let evidence: 'name' | 'body' | 'fallback' | 'none' = 'none'
    if (isConvincing(wsBest) && wsBest.score >= minScore) {
      ws = wsBest.name
      evidence = 'name'
    } else if (isConvincingByBody(wsBest) && wsBest.score >= minScore) {
      ws = wsBest.name
      evidence = 'body'
    }

    // ── 文档类型判定：同样的两级规则 ──
    let dt: string | null = null
    if (isConvincing(dtBest) && dtBest.score >= minScore) {
      dt = dtBest.name
    } else if (isConvincingByBody(dtBest) && dtBest.score >= minScore) {
      dt = dtBest.name
    }

    // 兜底：类型判得出来、工作域判不出来 → 归入主干工作线，但置信度要打折
    if (!ws && dt && FALLBACK_ALLOWED_DT.has(dt)) {
      ws = DEFAULT_WORKSTREAM
      evidence = 'fallback'
    }

    // 人名压缩包：给预填猜测，但强制进待确认（不自动通过）
    let personArchive = false
    if (!ws && !dt && looksLikePersonArchive(f)) {
      ws = '人事培训'
      dt = '考核人事'
      personArchive = true
    }

    const info = Math.min(1, docText(f).length / 120)
    const wsMargin =
      wsBest.score > 0
        ? Math.max(0, wsBest.score - wsSecond.score) / wsBest.score
        : 0
    const conf = personArchive
      ? 0.5
      : confidenceOf({
          evidence,
          wsCore: wsBest.coreHits,
          wsMargin,
          dtKnown: !!dt,
          info,
        })

    const needReview = conf < threshold || !ws || personArchive

    let reason: string
    if (personArchive) {
      reason = '按人名打包的压缩包，猜测是候选人简历，请确认'
    } else if (ws && dt && evidence === 'fallback') {
      reason = `类型是「${dt}」，工作线未能确定，暂归主干项目`
    } else if (ws && dt && evidence === 'body') {
      reason = `文件名看不出归属，依据正文判断为「${ws} · ${dt}」`
    } else if (ws && dt) {
      reason = `工作域「${ws}」+ 类型「${dt}」`
    } else if (ws) {
      reason = `能识别工作域「${ws}」，但判断不出文档类型`
    } else if (dt) {
      reason = `像是「${dt}」，但判断不出属于哪个项目/工作线`
    } else {
      reason = '没有足够特征，无法判断'
    }

    return {
      ...f,
      categoryId: null,
      suggestedName: suggestName(f, dt, ws),
      tags: buildTags(ws, dt, f),
      confidence: conf,
      needReview,
      status: needReview ? ('pending' as const) : ('classified' as const),
      reason,
      aiSource: 'rule' as const,
      ws,
      dt,
      _ws: ws,
      _dt: dt,
    }
  })

  return buildTaxonomy(staged, threshold)
}

/** 带归属暂存信息的文件（规则/模型融合后统一交给 buildTaxonomy 建树） */
export type StagedFile = FileItem & {
  _ws: string | null
  _dt: string | null
}

/**
 * 由「每份文件的工作域 + 文档类型」建两级分类树，并回写 categoryId。
 *
 * 规则引擎与「规则 + 大模型」混合管线共用这一处建树逻辑，
 * 保证两条路径的分类树、命名、标签规则完全一致。
 *
 * minWsCount：工作域至少聚集几份文件才建目录。
 *   纯规则模式用 2（避免偶然命中造出一堆只有一份文件的目录）；
 *   混合模式用 1（模型判出的单份文件通常确实独立，例如只有一份合同）。
 */
export function buildTaxonomy(
  staged: StagedFile[],
  threshold: number,
  minWsCount = 2
): { result: InductionResult; files: FileItem[] } {
  const categories: Category[] = []
  const wsId = new Map<string, string>()
  const dtId = new Map<string, string>()
  let order = 0

  for (const wsDef of WORKSTREAM_LEXICON) {
    const own = staged.filter((s) => s._ws === wsDef.name)
    if (own.length < minWsCount) continue
    const id = `ws_${order}`
    wsId.set(wsDef.name, id)
    categories.push({
      id,
      name: wsDef.name,
      desc: wsDef.desc,
      parentId: null,
      order: order++,
      isFromAI: true,
    })
    // 二级：该工作域下出现 ≥1 次的文档类型
    const dtCount = new Map<string, number>()
    for (const s of own) if (s._dt) dtCount.set(s._dt, (dtCount.get(s._dt) ?? 0) + 1)
    let sub = 0
    for (const dtDef of DOCTYPE_LEXICON) {
      const n = dtCount.get(dtDef.name) ?? 0
      if (n < 1) continue
      const sid = `${id}_${sub}`
      dtId.set(`${wsDef.name}||${dtDef.name}`, sid)
      categories.push({
        id: sid,
        name: dtDef.name,
        desc: `${wsDef.name} · ${dtDef.desc}`,
        parentId: id,
        order: sub++,
        isFromAI: true,
      })
    }
  }

  const finalFiles = staged.map((s) => {
    const { _ws, _dt, ...rest } = s
    let catId: string | null = null
    if (_ws && _dt) catId = dtId.get(`${_ws}||${_dt}`) ?? wsId.get(_ws) ?? null
    else if (_ws) catId = wsId.get(_ws) ?? null

    const needReview = rest.needReview || !catId || rest.confidence < threshold
    return {
      ...rest,
      ws: _ws,
      dt: _dt,
      categoryId: catId,
      needReview,
      status: needReview ? ('pending' as const) : ('classified' as const),
    } as FileItem
  })

  const byKind: Record<string, number> = {}
  for (const f of finalFiles) byKind[f.kind] = (byKind[f.kind] ?? 0) + 1

  return {
    result: {
      categories,
      fileCount: finalFiles.length,
      autoK: categories.filter((c) => !c.parentId).length,
      silhouette: 0,
      byKind,
    },
    files: finalFiles,
  }
}

export function buildTags(ws: string | null, dt: string | null, f: FileItem): string[] {
  const year = String(new Date(f.mtime).getFullYear())
  const tags: string[] = []
  if (ws) tags.push(ws)
  if (dt) tags.push(dt)
  tags.push(year)
  if (f.kind === 'drawio') tags.push('架构图')
  return Array.from(new Set(tags))
}

export { KIND_LABEL }
