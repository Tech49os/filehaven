<script setup lang="ts">
import { onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { executeMigration } from './core/migrator'
import type { MigrationPlan } from './core/types'
import { useDataStore } from './stores/data'
import { useWizardStore } from './stores/wizard'

const router = useRouter()
const w = useWizardStore()
const data = useDataStore()

/**
 * 开发/演示辅助：URL 带 ?auto=N 用示例数据直接跳到第 N 步
 *   例：/?auto=4         → 人工微调
 *       /?auto=5         → 迁移预览
 *       /?auto=main      → 跑完整流程后进主界面
 *       /?auto=main#/main/logs → 直接看迁移记录
 */
onMounted(async () => {
  // 先恢复持久化的东西，再决定进哪一步。
  // 注意 loadSettings 之前从来没被调用过——设置存进 IndexedDB 却从不读回来，
  // 表现就是「改完 Ollama 地址，重启程序又变回去」。
  await data.loadSettings()
  await w.loadSavedPlan()

  const auto = new URLSearchParams(location.search).get('auto')
  if (!auto || auto === '1') return

  await router.isReady()
  w.useDemo()

  if (auto === '2') {
    router.push('/wizard')
    w.goto(2)
    return
  }

  await w.startScan()
  await w.runProposal()
  w.confirmProposal()

  if (auto === 'main') {
    const plans: MigrationPlan[] = data.files
      .filter((f) => !f.needReview && f.categoryId)
      .map((f) => {
        const cat = data.categories.find((c) => c.id === f.categoryId)!
        const parent = cat.parentId
          ? data.categories.find((c) => c.id === cat.parentId)!
          : cat
        return {
          fileId: f.id,
          fileName: f.name,
          sourcePath: f.path,
          targetDir: cat.parentId ? `${parent.name}\\${cat.name}` : parent.name,
          targetName: f.suggestedName || f.name,
          conflict: false,
          deferred: false,
        }
      })
    const rep = await executeMigration(plans, {
      mode: 'demo',
      rootHandle: null,
      fileHandles: new Map(),
      batchId: `b_${Date.now()}`,
    })
    data.addRecords(rep.records)
    data.markMigrated(plans.map((p) => p.fileId))
    data.settings.rootPath = 'D:\\归档'
    data.settings.rootName = '归档'
    data.initialized = true
    const hash = location.hash.replace(/^#/, '')
    router.push(hash.startsWith('/main') ? hash : '/main/dashboard')
    return
  }

  const n = Number(auto)
  router.push('/wizard')
  if (Number.isFinite(n)) w.goto(Math.min(7, Math.max(1, n)))
})
</script>

<template>
  <div class="stage">
    <router-view />
  </div>
</template>
