/**
 * 文件名清洗与切段
 *
 * 真实语料里文件名长这样：
 *   附件2：双周工作情况报告-生产智能化项目组-1026.docx
 *   “智”造领航逐浪数字蓝海-集团V1.2.pptx
 *   04生产工艺设备服务器等硬件设备支撑（二期）_初版草稿_完.docx
 *
 * 两个反直觉的处理（都是原型实测暴露的）：
 *  1. 引号必须当「删除符」而不是「切分符」——否则「智造领航」会被切成
 *     「智」+「造领航」，事项名的第一个字直接丢掉。
 *  2. 中文括号同理，`（二期）` 应当与主体连成「支撑二期」，
 *     拆开会让「二期」变成孤立噪声。
 */

/** 扩展名 */
const EXT = /\.[A-Za-z0-9]{1,5}$/

/** 纯数字括号：ppt修改(1).pptx —— 整体删掉，避免留下一个孤零零的 1 */
const NUM_PAREN = /[（(]\s*\d+\s*[)）]/g

/** 引号与括号：删符号、保内容 */
const WRAP = /[“”"'’「」『』《》【】〔〕（）()\[\]]/g

const NOISE_RULES: RegExp[] = [
  /附件\s*\d+\s*[:：]?/g,                          // 附件2：
  /副本|最终版|最终|最新版|最新|修改版|修订版|定稿/g, // 版本口语
  /[Vv]\s?\d+(?:[._]\d+)*/g,                       // V1.0 v2
  /20\d{6}/g,                                      // 20260908
  /20\d{2}[-_.年]\d{1,2}(?:[-_.月]\d{1,2})?/g,      // 2026.8.4 / 2026-08
  /\d{2,}/g,                                       // 裸长数字
]

/** 切分符：这些是作者真正用来分段的符号 */
const SPLIT = /[-_–—\s+·、，,：:；;！!？?/\\|]+/

/**
 * 只剩数字/标点/符号的段没有意义。
 * 注意：不能用 `\W`——它在 JS 里把中文也归为非单词字符，
 * 会把「双周工作情况报告」整段判成无意义（原型第一次跑就踩了这个坑）。
 */
const MEANINGLESS = /^[\p{N}\p{P}\p{S}\s]+$/u

export function stripNoise(name: string): string {
  let s = name.replace(EXT, '')
  s = s.replace(NUM_PAREN, '|')
  s = s.replace(WRAP, '')
  for (const r of NOISE_RULES) s = s.replace(r, '|')
  // 收尾：去掉「初版草稿」「完」这类纯噪声尾词
  s = s.replace(/初版|草稿|^完|_完$/g, '|')
  return s.replace(/\|+/g, '|').replace(/^\||\|$/g, '').trim()
}

/** 清洗后按分隔符切段，去空、去重、去纯符号段 */
export function segments(name: string): string[] {
  const out: string[] = []
  for (const raw of stripNoise(name).split(SPLIT)) {
    const t = raw.trim()
    if (!t || MEANINGLESS.test(t)) continue
    if (!out.includes(t)) out.push(t)
  }
  return out
}
