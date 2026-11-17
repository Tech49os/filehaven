import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'
import { groupBySurface } from '../src/core/emergence/grouping'
import { DOC_KIND_WORDS } from '../src/core/emergence/lexicon'

const here = dirname(fileURLToPath(import.meta.url))
const data = JSON.parse(readFileSync(resolve(here, 'real-classify.json'), 'utf-8'))
const files = data.details.map((d: any) => ({ name: d.name, path: d.rel }))
const cands = collectCandidates(files, { topK: 60 })

const cn = (s: string) => (s.match(/[\u4e00-\u9fff]/g) ?? []).length
const useful = (term: string) => cn(term) >= 2 && !DOC_KIND_WORDS.has(term)

const groups = groupBySurface(cands.filter((c) => useful(c.term)))
const kept = groups.filter((g) => g.weight >= 3)
console.log(`候选 ${cands.length} → 分组 ${groups.length} → 提案 ${kept.length} 项\n`)
kept.forEach((g, i) => {
  console.log(`${String(i + 1).padStart(2)}. ${g.name}  ｜${g.aliases.join('、')}｜权重 ${g.weight}`)
})
const dropped = groups.filter((g) => g.weight < 3).map((g) => g.name)
console.log(`\n被筛掉（权重<3）：${dropped.join('、')}`)
writeFileSync(resolve(here, 'groups-real.json'), JSON.stringify(kept, null, 1), 'utf-8')
