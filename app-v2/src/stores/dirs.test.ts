import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { ArchiveDir, DirNode } from '../core/types'

const listDirs = vi.fn()
const createDir = vi.fn()
vi.mock('../core/ipc', () => ({ listDirs, createDir }))

const { useDirsStore } = await import('./dirs')

/** 极简假后端：按 relPath 拼树，够验证「建 A、A/B 后两层可见」 */
let rows: ArchiveDir[] = []
function nodeOf(r: ArchiveDir): DirNode {
  return {
    id: r.id,
    name: r.name,
    relPath: r.relPath,
    depth: r.depth,
    origin: r.origin,
    fileCount: 0,
    children: [],
  }
}
function buildTree(): DirNode[] {
  const roots: DirNode[] = []
  const byPath = new Map<string, DirNode>()
  for (const r of [...rows].sort((a, b) => a.relPath.localeCompare(b.relPath))) {
    const n = nodeOf(r)
    byPath.set(r.relPath, n)
    const slash = r.relPath.lastIndexOf('/')
    if (slash < 0) {
      roots.push(n)
    } else {
      byPath.get(r.relPath.slice(0, slash))?.children.push(n)
    }
  }
  return roots
}

describe('dirs store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    rows = []
    listDirs.mockReset().mockImplementation(async () => buildTree())
    createDir.mockReset().mockImplementation(async (input: string) => {
      const segs = input.split('/')
      let rel = ''
      let lastId: number | null = null
      for (let i = 0; i < segs.length; i += 1) {
        rel = rel ? `${rel}/${segs[i]}` : segs[i]
        let row = rows.find((r) => r.relPath === rel)
        if (!row) {
          row = {
            id: rows.length + 1,
            parentId: lastId,
            name: segs[i],
            relPath: rel,
            depth: i + 1,
            origin: 'user',
            createdAt: 0,
            lastUsedAt: null,
          }
          rows.push(row)
        }
        lastId = row.id
      }
      const created = rows.find((r) => r.relPath === rel)
      if (!created) throw new Error('假后端没建出来')
      return created
    })
  })

  it('load 后树里能看到两层（建 A、A/B）', async () => {
    const s = useDirsStore()
    await s.createDir('A')
    await s.createDir('A/B')
    expect(s.tree).toHaveLength(1)
    expect(s.tree[0].name).toBe('A')
    expect(s.tree[0].children).toHaveLength(1)
    expect(s.tree[0].children[0].name).toBe('B')
    expect(s.count).toBe(2)
    expect(s.allPaths).toEqual(['A', 'A/B'])
  })

  it('createDir 之后自动选中新目录', async () => {
    const s = useDirsStore()
    const d = await s.createDir('安全检查')
    expect(s.selectedId).toBe(d.id)
    expect(s.selected?.relPath).toBe('安全检查')
  })

  it('select 切换选中态，传 null 取消选中', async () => {
    const s = useDirsStore()
    await s.createDir('A')
    await s.createDir('B')
    const [a, b] = s.allPaths
    s.select(s.tree[0].id)
    expect(s.selected?.relPath).toBe(a)
    s.select(s.tree[1].id)
    expect(s.selected?.relPath).toBe(b)
    s.select(null)
    expect(s.selected).toBeNull()
  })

  it('load 失败落进 error，不抛到页面', async () => {
    const s = useDirsStore()
    listDirs.mockRejectedValueOnce({ code: 'DB', message: '读目录失败' })
    await s.load()
    expect(s.error).toBe('读目录失败')
    expect(s.tree).toEqual([])
  })
})
