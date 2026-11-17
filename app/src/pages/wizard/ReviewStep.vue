<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import ConfidenceBadge from '../../components/ConfidenceBadge.vue'
import PendingQueue from '../../components/PendingQueue.vue'
import { useDataStore } from '../../stores/data'
import { useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const data = useDataStore()

const selected = ref<string>('')
const tagInput = ref('')

const form = reactive({
  categoryId: '',
  name: '',
  tags: [] as string[],
})

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

onMounted(() => {
  selected.value = data.pendingFiles[0]?.id ?? ''
})

const catOptions = computed(() => {
  const out: { id: string; label: string }[] = []
  for (const root of data.categoryTree) {
    out.push({ id: root.id, label: root.name })
    for (const c of root.children) {
      out.push({ id: c.id, label: `${root.name} › ${c.name}` })
    }
  }
  return out
})

const withGuess = computed(() => data.pendingFiles.filter((f) => f.categoryId).length)

const targetPath = computed(() => {
  if (!form.categoryId) return `${w.archivePath}\\（原处不动）`
  const label = catOptions.value.find((o) => o.id === form.categoryId)?.label ?? ''
  return `${w.archivePath}\\${label.replace(' › ', '\\')}`
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
  const nextOne = list[idx + 1] ?? list.find((f) => f.id !== selected.value)
  selected.value = nextOne?.id ?? ''
}

function acceptAll() {
  const targets = data.pendingFiles.filter((f) => f.categoryId)
  for (const f of targets) {
    data.confirmFile(f.id, f.categoryId!, f.suggestedName || f.name, f.tags)
  }
  selected.value = data.pendingFiles[0]?.id ?? ''
}

function addTag() {
  const t = tagInput.value.trim()
  if (t && !form.tags.includes(t)) form.tags.push(t)
  tagInput.value = ''
}

function removeTag(t: string) {
  form.tags = form.tags.filter((x) => x !== t)
}

function next() {
  w.goto(5)
}
</script>

<template>
  <div class="step">
    <div class="step-body">
      <div class="head">
        <div>
          <h1>人工微调</h1>
          <p class="sub">
            AI 拿不准的部分交给你。确认后即成为后续自动整理的规则。
          </p>
        </div>
        <button
          v-if="withGuess"
          class="btn btn-secondary btn-sm"
          @click="acceptAll"
        >
          接受 AI 建议（{{ withGuess }}）
        </button>
      </div>

      <div class="grid">
        <div class="panel queue-panel">
          <PendingQueue
            :files="data.pendingFiles"
            :selected="selected"
            explain="置信度低于 70% 的文件不会硬猜，留给你判断"
            @select="selected = $event"
          />
        </div>

        <div class="panel edit-panel">
          <template v-if="current">
            <div class="scroll inner">
              <div class="file-head">
                <div class="fi">{{ current.kind === 'drawio' ? 'IO' : current.ext.toUpperCase().slice(0, 4) }}</div>
                <div class="fmain">
                  <div class="fname ellipsis">{{ current.name }}</div>
                  <div class="fpath ellipsis">{{ current.path }}</div>
                </div>
                <ConfidenceBadge :value="current.confidence" />
              </div>

              <div class="preview">
                <div class="p-label">
                  <AppIcon name="info" :size="12" />
                  内容预览 · AI 抽取
                </div>
                <div class="p-text">
                  <template v-if="current.drawioNodes?.length">
                    <div class="nodes">
                      <span v-for="n in current.drawioNodes" :key="n" class="node">{{ n }}</span>
                    </div>
                  </template>
                  <template v-else-if="current.textSummary">
                    {{ current.textSummary }}
                  </template>
                  <template v-else>
                    <span class="muted">没有可读取的正文（可能是图片、扫描件或空文件）</span>
                  </template>
                </div>
              </div>

              <div class="ai-hint">
                <AppIcon name="sparkle" :size="13" />
                <span>{{ current.reason || 'AI 无法判断这份文件属于哪一类' }}</span>
              </div>

              <div class="form">
                <label class="block">
                  <span class="form-label">归档分类</span>
                  <select v-model="form.categoryId" class="field">
                    <option value="">（不归档，留在原处）</option>
                    <option v-for="o in catOptions" :key="o.id" :value="o.id">
                      {{ o.label }}
                    </option>
                  </select>
                </label>

                <label class="block">
                  <span class="form-label">归档文件名</span>
                  <input v-model="form.name" class="field" spellcheck="false" />
                </label>

                <div class="block">
                  <span class="form-label">标签</span>
                  <div class="tags">
                    <span v-for="t in form.tags" :key="t" class="tag">
                      {{ t }}
                      <button class="tag-close" @click="removeTag(t)">×</button>
                    </span>
                    <input
                      v-model="tagInput"
                      class="tag-input"
                      placeholder="+ 添加标签"
                      @keydown.enter.prevent="addTag"
                    />
                  </div>
                </div>

                <div class="block">
                  <span class="form-label">目标位置</span>
                  <div class="target">
                    <code>{{ targetPath }}</code>
                  </div>
                </div>
              </div>
            </div>

            <div class="edit-foot">
              <button class="btn btn-muted btn-sm" @click="skip">跳过</button>
              <div class="spacer" />
              <button class="btn btn-secondary btn-sm" @click="apply">应用并下一个</button>
              <button class="btn btn-primary btn-sm" @click="apply">确认分类</button>
            </div>
          </template>

          <div v-else class="done-all">
            <div class="da-ico"><AppIcon name="check" :size="22" :stroke="2.4" /></div>
            <h3>待确认队列已清空</h3>
            <p class="muted tiny">剩下的文件 AI 都有把握，可以直接预览迁移计划。</p>
          </div>
        </div>
      </div>
    </div>

    <div class="step-foot">
      <button class="btn btn-text btn-sm" @click="w.goto(3)">
        <AppIcon name="arrow-left" :size="13" />
        返回 AI 归纳
      </button>
      <div class="spacer" />
      <span class="muted tiny">
        剩余 {{ data.pendingFiles.length }} 个 · 可稍后处理
      </span>
      <button class="btn btn-primary" @click="next">
        预览迁移计划
        <AppIcon name="arrow-right" :size="14" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  flex: 0 0 auto;
}
h1 {
  font-size: 22px;
  font-weight: 500;
  letter-spacing: -0.3px;
  margin: 0;
}
.sub {
  font-size: 12.5px;
  color: var(--c-text-2);
  margin: 5px 0 0;
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

.edit-panel {
  min-height: 0;
}
.inner {
  flex: 1;
  min-height: 0;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.file-head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 0 0 auto;
}
.fi {
  width: 32px;
  height: 32px;
  border-radius: 9px;
  background: var(--c-primary-bg);
  color: var(--c-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 500;
  flex: 0 0 32px;
}
.fmain {
  flex: 1;
  min-width: 0;
}
.fname {
  font-size: 13.5px;
  font-weight: 500;
}
.fpath {
  font-size: 11px;
  color: var(--c-text-3);
}

.preview {
  background: var(--c-field);
  border-radius: var(--r-banner);
  padding: 11px 13px;
  flex: 0 0 auto;
}
.p-label {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--c-text-2);
  margin-bottom: 6px;
}
.p-text {
  font-size: 12px;
  line-height: 1.65;
  color: var(--c-text);
  max-height: 92px;
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

.ai-hint {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
  border-radius: var(--r-banner);
  padding: 9px 12px;
  font-size: 12px;
  line-height: 1.55;
  flex: 0 0 auto;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-top: 2px;
}
.block {
  display: block;
}
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}
.tag-input {
  border: none;
  background: transparent;
  font-size: 12px;
  color: var(--c-primary);
  padding: 5px 4px;
  min-width: 88px;
}
.tag-input::placeholder {
  color: var(--c-primary);
  opacity: 0.75;
}
.target code {
  display: block;
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 11.5px;
  color: var(--c-text-2);
  background: var(--c-field);
  border-radius: var(--r-item);
  padding: 8px 11px;
  word-break: break-all;
}

.edit-foot {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 11px 18px;
  border-top: 0.5px solid var(--c-border);
  background: rgba(0, 0, 0, 0.012);
}

.done-all {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 30px;
}
.da-ico {
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
.done-all h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 500;
}

.step-body {
  gap: 12px;
}
</style>
