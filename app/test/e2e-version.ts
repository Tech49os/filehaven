/**
 * 用真实 455 份文件体检版本族识别。
 * 只读 test/desktop-scan.json，不碰真实磁盘。
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { buildFamilies, familyRenames, type FamFile } from '../src/core/version'

const raw = JSON.parse(readFileSync('test/desktop-scan.json', 'utf8'))
const files: FamFile[] = (raw.files ?? raw).map((f: { path: string; name: string; size: number; mtime: number }) => ({
  id: f.path,
  name: f.name,
  size: f.size,
  mtime: f.mtime,
}))

const fams = buildFamilies(files)
const inFam = fams.reduce((n, f) => n + f.files.length, 0)
const renames = familyRenames(files)

console.log(`文件 ${files.length} / 族 ${fams.length} / 族内文件 ${inFam} (${((inFam / files.length) * 100).toFixed(1)}%) / 会改名 ${renames.size}`)
console.log(`族大小分布: ${[2, 3, 4, 5].map((n) => `${n}份×${fams.filter((f) => f.files.length === n).length}`).join('  ')} 更多×${fams.filter((f) => f.files.length > 5).length}`)

for (const fam of [...fams].sort((a, b) => b.files.length - a.files.length)) {
  const exts = [...new Set(fam.files.map((f) => f.name.slice(f.name.lastIndexOf('.'))))]
  console.log(`\n[${fam.files.length}] ${fam.title}  ${exts.join('/')}`)
  for (const f of fam.files) {
    const to = renames.get(f.id)
    console.log(`    ${new Date(f.mtime).toISOString().slice(0, 10)}  ${(f.size / 1024).toFixed(0)}KB  ${f.name}${to ? `  →  ${to}` : ''}`)
  }
}

writeFileSync('test/e2e-version.json', JSON.stringify({ count: fams.length, renames: Object.fromEntries(renames) }, null, 1))
