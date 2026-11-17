import { defineStore } from 'pinia'
import type {
  AppSettings,
  Category,
  CategoryNode,
  FileItem,
  MigrationRecord,
} from '../core/types'
import { dbGet, dbSet } from '../core/db'

export const useDataStore = defineStore('data', {
  state: () => ({
    files: [] as FileItem[],
    categories: [] as Category[],
    records: [] as MigrationRecord[],
    initialized: false,
    settings: {
      rootPath: 'D:\\文档归档',
      rootName: '文档归档',
      mode: 'demo',
      confidenceThreshold: 0.7,
      autoOrganizeEnabled: true,
      idleThresholdSec: 600,
      requirePower: true,
      maxFileSizeMB: 2048,
      // ===== 本地大模型（Ollama）=====
      llmEnabled: true,
      llmBaseUrl: 'http://127.0.0.1:11434',
      llmModel: 'qwen2.5:7b',
      llmApiKey: '',
      llmConcurrency: 2,
      llmStrategy: 'smart',
      llmMaxFiles: 60,
    } as AppSettings,
  }),

  getters: {
    totalFiles: (s) => s.files.length,

    pendingFiles: (s) =>
      s.files.filter((f) => f.needReview && f.status !== 'migrated'),

    archivedFiles: (s) => s.files.filter((f) => f.status === 'migrated'),

    /** 分类树（两级） */
    categoryTree(s): CategoryNode[] {
      const roots = s.categories
        .filter((c) => !c.parentId)
        .sort((a, b) => a.order - b.order)
      return roots.map((r) => {
        const children = s.categories.filter((c) => c.parentId === r.id)
        const childNodes: CategoryNode[] = children.map((c) => ({
          ...c,
          children: [],
          count: s.files.filter((f) => f.categoryId === c.id).length,
          totalCount: s.files.filter((f) => f.categoryId === c.id).length,
        }))
        const direct = s.files.filter((f) => f.categoryId === r.id).length
        return {
          ...r,
          children: childNodes,
          count: direct,
          totalCount: direct + childNodes.reduce((a, c) => a + c.count, 0),
        }
      })
    },

    stats(s) {
      const weekAgo = Date.now() - 7 * 86400000
      const bytes = s.files.reduce((a, f) => a + f.size, 0)
      const archived = s.files.filter((f) => f.status === 'migrated').length
      const pending = s.files.filter(
        (f) => f.needReview && f.status !== 'migrated'
      ).length
      const week = s.records.filter((r) => r.time >= weekAgo).length
      return {
        total: s.files.length,
        archived,
        pending,
        week,
        bytes,
        gb: bytes / 1024 / 1024 / 1024,
      }
    },
  },

  actions: {
    setResult(files: FileItem[], categories: Category[]) {
      this.files = files
      this.categories = categories
    },

    updateFile(id: string, patch: Partial<FileItem>) {
      const i = this.files.findIndex((f) => f.id === id)
      if (i >= 0) this.files[i] = { ...this.files[i], ...patch }
    },

    confirmFile(id: string, categoryId: string, name: string, tags: string[]) {
      const i = this.files.findIndex((f) => f.id === id)
      if (i < 0) return
      this.files[i] = {
        ...this.files[i],
        categoryId,
        suggestedName: name,
        tags,
        confidence: 1,
        needReview: false,
        status: 'confirmed',
      }
    },

    markMigrated(ids: string[]) {
      for (const id of ids) {
        const i = this.files.findIndex((f) => f.id === id)
        if (i >= 0) {
          this.files[i] = { ...this.files[i], status: 'migrated', needReview: false }
        }
      }
    },

    addRecords(records: MigrationRecord[]) {
      this.records = [...records, ...this.records]
    },

    markReverted(batchId: string) {
      this.records = this.records.map((r) =>
        r.batchId === batchId && r.status === 'recycled'
          ? { ...r, status: 'reverted' }
          : r
      )
    },

    async saveSettings() {
      await dbSet('settings', this.settings)
    },

    async loadSettings() {
      const s = await dbGet<AppSettings>('settings')
      if (s) this.settings = { ...this.settings, ...s }
      // 旧版本把 LLM 指向 OpenAI 兼容接口（当时其实从未被调用），
      // 现在改为本地 Ollama，旧值会让"启用大模型"永远连不上，这里做一次迁移
      if (/openai\.com|gpt-/i.test(this.settings.llmBaseUrl + this.settings.llmModel)) {
        this.settings.llmBaseUrl = 'http://127.0.0.1:11434'
        this.settings.llmModel = 'qwen2.5:7b'
      }
    },
  },
})
