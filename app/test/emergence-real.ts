import { readFileSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { collectCandidates } from '../src/core/emergence/candidates'

const here = dirname(fileURLToPath(import.meta.url))
const data = JSON.parse(readFileSync(resolve(here, 'real-classify.json'), 'utf-8'))
const files = data.details.map((d: any) => ({ name: d.name, path: d.rel }))

const cs = collectCandidates(files, { topK: 200 })
console.log(`文件 ${files.length} 份 → 候选 ${cs.length} 个\n`)
console.log('序号  DF  组织  专名')
cs.slice(0, 100).forEach((c, i) => {
  console.log(`${String(i + 1).padStart(3)}  ${String(c.df).padStart(3)}  ${c.isOrg ? ' org ' : '     '} ${c.term}`)
})
writeFileSync(resolve(here, 'candidates-real.json'), JSON.stringify(cs, null, 1), 'utf-8')
