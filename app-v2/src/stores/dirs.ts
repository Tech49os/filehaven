/**
 * 归档目录树（T6.8）
 *
 * P1 只读 + 点击选中，不做拖拽；写入只有 `createDir` 一条路。
 */

import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import * as ipc from '../core/ipc'
import { toCmdError } from '../core/cmdError'
import type { ArchiveDir, DirNode } from '../core/types'

function flatten(nodes: DirNode[], out: DirNode[] = []): DirNode[] {
  for (const n of nodes) {
    out.push(n)
    flatten(n.children, out)
  }
  return out
}

export const useDirsStore = defineStore('dirs', () => {
  const tree = ref<DirNode[]>([])
  const selectedId = ref<number | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const flat = computed(() => flatten(tree.value))
  const count = computed(() => flat.value.length)
  const allPaths = computed(() => flat.value.map((d) => d.relPath))
  const selected = computed(() =>
    selectedId.value === null
      ? null
      : (flat.value.find((d) => d.id === selectedId.value) ?? null),
  )

  async function load() {
    loading.value = true
    try {
      tree.value = await ipc.listDirs()
      error.value = null
      // 选中的目录可能已被删/重建
      if (selectedId.value !== null && !flat.value.some((d) => d.id === selectedId.value)) {
        selectedId.value = null
      }
    } catch (e) {
      error.value = toCmdError(e).message
      tree.value = []
    } finally {
      loading.value = false
    }
  }

  async function createDir(input: string): Promise<ArchiveDir> {
    const d = await ipc.createDir(input)
    await load()
    selectedId.value = d.id
    return d
  }

  function select(id: number | null) {
    selectedId.value = id
  }

  return { tree, selectedId, selected, loading, error, flat, count, allPaths, load, createDir, select }
})
