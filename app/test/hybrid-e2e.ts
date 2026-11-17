/**
 * 混合归纳端到端测试（真实调用本地 Ollama）
 *
 * 用内置的 73 份真实业务样本跑一遍「规则 + 大模型」管线，输出：
 *   - 送模型 / 一致 / 补判 / 分歧 的分布
 *   - 与样本标注（ws/dt）对比的准确率，规则 vs 混合
 *   - 耗时与单份平均耗时
 *
 * 运行：npx tsx test/hybrid-e2e.ts [模型名]
 */
import { ALL_SAMPLES } from '../src/core/sample'
import { induceHybrid } from '../src/core/hybrid'
import { induce } from '../src/core/classifier'
import { extOf } from '../src/core/extractor'
import type { FileItem } from '../src/core/types'

const model = process.argv[2] ?? 'qwen2.5:7b'
const now = Date.now()

const files: FileItem[] = ALL_SAMPLES.map((s, i) => ({
  id: `s${i}`,
  name: s.name,
  ext: extOf(s.name),
  kind: s.kind,
  size: s.size * 1024,
  mtime: now - s.daysAgo * 86400000,
  path: `D:\\演示\\${s.name}`,
  textSummary: s.content,
  drawioNodes: s.drawioNodes,
  categoryId: null,
  suggestedName: '',
  tags: [],
  confidence: 0,
  needReview: true,
  status: 'scanned',
}))

const truth = new Map(ALL_SAMPLES.map((s, i) => [`s${i}`, s]))

function accuracy(list: FileItem[]) {
  let wsOk = 0
  let dtOk = 0
  let wsTotal = 0
  let dtTotal = 0
  let wrongWs: string[] = []
  for (const f of list) {
    const t = truth.get(f.id)!
    if (t.ws) {
      wsTotal++
      if (f.ws === t.ws) wsOk++
      else wrongWs.push(`${f.name} 判成「${f.ws ?? '空'}」，应为「${t.ws}」（${f.aiSource}）`)
    }
    if (t.dt) {
      dtTotal++
      if (f.dt === t.dt) dtOk++
    }
  }
  return {
    ws: wsTotal ? Math.round((wsOk / wsTotal) * 1000) / 10 : 0,
    dt: dtTotal ? Math.round((dtOk / dtTotal) * 1000) / 10 : 0,
    wrongWs,
  }
}

const t0 = Date.now()
const ruleBase = induce(files, { threshold: 0.7 })
const ruleAcc = accuracy(ruleBase.files)
console.log(
  `\n[规则基线] ws ${ruleAcc.ws}% · dt ${ruleAcc.dt}% · 耗时 ${Date.now() - t0}ms`
)

let last = 0
const { files: out, stats } = await induceHybrid(files, {
  threshold: 0.7,
  llm: {
    enabled: true,
    config: { baseUrl: 'http://127.0.0.1:11434', model, apiKey: '' },
    strategy: 'smart',
    concurrency: 2,
    maxFiles: 0,
  },
  onProgress: (p) => {
    if (p.phase !== 'llm') {
      if (p.current) process.stdout.write(`\r${p.phase}: ${p.current}`.padEnd(90).slice(0, 90))
      return
    }
    if (p.done - last >= 5 || p.done === p.total) {
      last = p.done
      const eta = p.done ? Math.round(((p.total - p.done) * (Date.now() - t0)) / p.done / 1000) : 0
      process.stdout.write(
        `\r模型复核 ${p.done}/${p.total} · 约剩 ${eta}s`.padEnd(60).slice(0, 60)
      )
    }
  },
})

const hybridAcc = accuracy(out)
console.log('\n')
console.log('=== 统计 ===')
console.log(JSON.stringify(stats, null, 2))
console.log(
  `\n[混合结果] ws ${hybridAcc.ws}% · dt ${hybridAcc.dt}% · ` +
    `待确认 ${out.filter((f) => f.needReview).length}/${out.length}`
)
console.log('\n=== 规则判错、混合纠正的样本 ===')
const ruleWrong = new Set(ruleAcc.wrongWs.map((s) => s.split(' 判成')[0]))
for (const line of hybridAcc.wrongWs) {
  const name = line.split(' 判成')[0]
  const tag = ruleWrong.has(name) ? '（规则也错）' : '（规则本来对）'
  console.log(' -', line, tag)
}
const fixed = ruleAcc.wrongWs.filter((l) => !hybridAcc.wrongWs.includes(l))
console.log(`\n规则判错 ${ruleAcc.wrongWs.length} 份，混合后 ${hybridAcc.wrongWs.length} 份`)
console.log(`其中被模型纠正 ${Math.max(0, ruleAcc.wrongWs.length - hybridAcc.wrongWs.length)} 份`)
void fixed
