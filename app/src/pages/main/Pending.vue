<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import ConfidenceBadge from '../../components/ConfidenceBadge.vue'
import PendingQueue from '../../components/PendingQueue.vue'
import { useDataStore } from '../../stores/data'

const data = useDataStore()
const selected = ref('')
const form = reactive({ categoryId: '', name: '', tags: [] as string[] })

const current = computed(() => {
  const list = data.pendingFiles
  return list.find((f) => f.id === selected.value) ?? list[0] ?? null
})

watch(
  current,
  (f) => {
    if (!f) return
    if (f.id !== selected.value) selected.value = f.id
    form.categoryId = f.categoryId ?? ''
    form.name = f.suggestedName || f.name
    form.tags = [...f.tags]
  },
  { immediate: true }
)

const catOptions = computed(() => {
  const out: { id: string; label: string }[] = []
  for (const root of data.categoryTree) {
    out.push({ id: root.id, label: root.name })
    for (const c of root.children) out.push({ id: c.id, label: `${root.name} › ${c.name}` })
  }
  return out
})

function apply() {
  const f = current.value
  if (!f) return
  data.confirmFile(f.id, form.categoryId, form.name.trim() || f.name, form.tags)
  selected.value = data.pendingFiles[0]?.id ?? ''
}

function skip() {
  const list = data.pendingFiles
  const idx = list.findIndex((f) => f.id === selected.value)
  selected.value = (list[idx + 1] ?? list.find((f) => f.id !== selected.value))?.id ?? ''
}
</script>

<template>
  <div class="page">
    <header class="top">
      <div>
        <h1 class="page-title">待确认</h1>
        <p class="page-sub">
          置信度低于 {{ Math.round(data.settings.confidenceThreshold * 100) }}% 的文件不会硬猜，交给你判断
        </p>
      </div>
    </header>

    <div class="grid">
      <div class="panel">
        <PendingQueue
          :files="data.pendingFiles"
          :selected="selected"
          @select="selected = $event"
        />
      </div>

      <div class="panel edit">
        <template v-if="current">
          <div class="scroll inner">
            <div class="fh">
              <div class="nm">{{ current.name }}</div>
              <ConfidenceBadge :value="current.confidence" />
            </div>
            <div class="path ellipsis">{{ current.path }}</div>

            <div class="pv">
              <div class="pv-label">内容预览 · AI 抽取</div>
              <div v-if="current.drawioNodes?.length" class="nodes">
                <span v-for="n in current.drawioNodes" :key="n" class="node">{{ n }}</span>
              </div>
              <div v-else-if="current.textSummary" class="pv-text">
                {{ current.textSummary }}
              </div>
              <div v-else class="muted">没有可读取的正文（图片 / 扫描件 / 空文件）</div>
            </div>

            <div class="hint">
              <AppIcon name="sparkle" :size="13" />
              <span>{{ current.reason || 'AI 无法判断这份文件属于哪一类' }}</span>
            </div>

            <div class="form">
              <label class="block">
                <span class="form-label">归档分类</span>
                <select v-model="form.categoryId" class="field">
                  <option value="">（不归档）</option>
                  <option v-for="o in catOptions" :key="o.id" :value="o.id">{{ o.label }}</option>
                </select>
              </label>
              <label class="block">
                <span class="form-label">归档文件名</span>
                <input v-model="form.name" class="field" spellcheck="false" />
              </label>
            </div>
          </div>
          <div class="ef">
            <button class="btn btn-muted btn-sm" @click="skip">跳过</button>
            <div class="spacer" />
            <button class="btn btn-primary btn-sm" @click="apply">确认并下一个</button>
          </div>
        </template>
        <div v-else class="clear">
          <div class="ck"><AppIcon name="check" :size="22" :stroke="2.4" /></div>
          <h3>全部确认完毕</h3>
          <p class="muted tiny">新文件会在空闲整理时自动归类，拿不准的仍会出现在这里。</p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page {
  flex: 1;
  min-height: 0;
  padding: var(--pad-content);
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.top {
  flex: 0 0 auto;
}
.grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 258px minmax(0, 1fr);
  gap: 16px;
}
.panel {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.inner {
  flex: 1;
  min-height: 0;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 11px;
}
.fh {
  display: flex;
  align-items: center;
  gap: 10px;
}
.nm {
  font-size: 14px;
  font-weight: 500;
  flex: 1;
  min-width: 0;
}
.path {
  font-size: 11.5px;
  color: var(--c-text-3);
}
.pv {
  background: var(--c-field);
  border-radius: var(--r-banner);
  padding: 11px 13px;
}
.pv-label {
  font-size: 11px;
  color: var(--c-text-2);
  margin-bottom: 6px;
}
.pv-text {
  font-size: 12px;
  line-height: 1.65;
  max-height: 110px;
  overflow: auto;
}
.nodes {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.node {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
  border-radius: 6px;
  padding: 2px 8px;
  font-size: 11px;
}
.hint {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
  border-radius: var(--r-banner);
  padding: 9px 12px;
  font-size: 12px;
  line-height: 1.55;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.ef {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 11px 18px;
  border-top: 0.5px solid var(--c-border);
}
.clear {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
}
.ck {
  width: 46px;
  height: 46px;
  border-radius: 50%;
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 4px;
}
.clear h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 500;
}
</style>
