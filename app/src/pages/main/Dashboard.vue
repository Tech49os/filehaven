<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import Card from '../../components/Card.vue'
import FileListItem from '../../components/FileListItem.vue'
import IdleIndicator from '../../components/IdleIndicator.vue'
import StatCard from '../../components/StatCard.vue'
import { useDataStore } from '../../stores/data'

const data = useDataStore()
const router = useRouter()

const stats = computed(() => data.stats)

const sizeText = computed(() => {
  const gb = stats.value.gb
  return gb >= 1 ? gb.toFixed(1) : (stats.value.bytes / 1024 / 1024).toFixed(0)
})
const sizeUnit = computed(() => (stats.value.gb >= 1 ? 'GB' : 'MB'))

const recent = computed(() =>
  [...data.files]
    .filter((f) => f.status === 'migrated')
    .sort((a, b) => b.mtime - a.mtime)
    .slice(0, 6)
)

const pendingPreview = computed(() => data.pendingFiles.slice(0, 6))

/** 待确认文件显示 AI 的猜测分类，而不是重复的路径 */
function guessLabel(f: (typeof data.files)[number]): string {
  if (!f.categoryId) return 'AI 无法判断'
  const name = data.categories.find((c) => c.id === f.categoryId)?.name
  return name ? `猜测：${name}` : 'AI 无法判断'
}

const isEmpty = computed(() => data.files.length === 0)

const idleStatus = computed<'idle' | 'paused'>(() =>
  data.settings.autoOrganizeEnabled ? 'idle' : 'paused'
)
</script>

<template>
  <div class="page scroll">
    <header class="top">
      <div>
        <h1 class="page-title">概览</h1>
        <p class="page-sub">
          {{ isEmpty ? '还没有整理过文件' : `${data.files.length} 个文件已纳入管理` }}
        </p>
      </div>
      <IdleIndicator :status="idleStatus" />
    </header>

    <!-- 空状态 -->
    <div v-if="isEmpty" class="empty">
      <div class="e-ico"><AppIcon name="folder" :size="26" /></div>
      <h3>先去整理一次文件</h3>
      <p class="muted tiny">运行初始化向导，AI 会归纳出你的分类体系并完成归档。</p>
      <button class="btn btn-primary" @click="router.push('/wizard')">
        开始初始化
      </button>
    </div>

    <template v-else>
      <div class="stats">
        <StatCard label="已归档" :value="stats.archived" unit="个文件" tone="success" />
        <StatCard
          label="待确认"
          :value="stats.pending"
          unit="个"
          :tone="stats.pending ? 'warning' : 'default'"
        >
          <template #sub>{{ stats.pending ? '需要你确认分类' : '队列已清空' }}</template>
        </StatCard>
        <StatCard label="本周整理" :value="stats.week" unit="个" tone="primary" />
        <StatCard label="库总占用" :value="sizeText" :unit="sizeUnit" />
      </div>

      <div class="grid">
        <Card title="最近归档" flush>
          <template #action>
            <button class="btn btn-text btn-sm" @click="router.push('/main/logs')">
              迁移记录
            </button>
          </template>
          <div v-if="recent.length" class="list">
            <FileListItem
              v-for="f in recent"
              :key="f.id"
              :file="f"
              :show-badge="false"
              :subtitle="`${data.categories.find((c) => c.id === f.categoryId)?.name ?? '未分类'} · ${f.suggestedName}`"
            />
          </div>
          <div v-else class="blank">还没有归档记录</div>
        </Card>

        <Card title="待确认队列" flush>
          <template #action>
            <button class="btn btn-text btn-sm" @click="router.push('/main/pending')">
              去处理
            </button>
          </template>
          <div v-if="pendingPreview.length" class="list">
            <FileListItem
              v-for="f in pendingPreview"
              :key="f.id"
              :file="f"
              :subtitle="guessLabel(f)"
            />
          </div>
          <div v-else class="blank">
            <div class="blank-ok"><AppIcon name="check" :size="16" :stroke="2.4" /></div>
            全部文件都已确认
          </div>
        </Card>
      </div>
    </template>
  </div>
</template>

<style scoped>
.page {
  flex: 1;
  min-height: 0;
  padding: var(--pad-content);
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.top {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  flex: 0 0 auto;
}

.stats {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 14px;
  flex: 0 0 auto;
}

.grid {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
  gap: 16px;
  flex: 1;
  min-height: 0;
}

.list {
  overflow: auto;
  min-height: 0;
}

.blank {
  padding: 30px 18px;
  text-align: center;
  font-size: 12px;
  color: var(--c-text-3);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
}
.blank-ok {
  width: 30px;
  height: 30px;
  border-radius: 50%;
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
}
.e-ico {
  width: 56px;
  height: 56px;
  border-radius: 18px;
  background: var(--c-primary-bg);
  color: var(--c-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 6px;
}
.empty h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 500;
}
.empty p {
  margin: 0 0 12px;
}
</style>
