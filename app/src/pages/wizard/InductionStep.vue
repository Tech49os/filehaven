<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import CategoryTree from '../../components/CategoryTree.vue'
import FileListItem from '../../components/FileListItem.vue'
import SegmentedControl from '../../components/SegmentedControl.vue'
import { useDataStore } from '../../stores/data'
import { useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const data = useDataStore()

const selected = ref<string | null>(null)
const mode = ref('all')
const ready = ref(false)
const now = ref(Date.now())
let timer: ReturnType<typeof setInterval> | null = null

const SOURCE_LABEL: Record<string, string> = {
  agree: '双路一致',
  llm: '模型补判',
  conflict: '双路分歧',
  rule: '规则判定',
  none: '未能判断',
}

const PHASE_TEXT: Record<string, string> = {
  rules: '规则引擎正在用词典证据快速初判…',
  warmup: '正在把本地大模型加载进显存（首次约 10 秒）…',
  llm: '本地大模型正在逐份阅读文件…',
  tree: '正在按归属生成两级分类目录…',
  done: '归纳完成',
}

async function run() {
  ready.value = false
  now.value = Date.now()
  try {
    await w.runInduction()
  } finally {
    ready.value = true
  }
}

onMounted(() => {
  timer = setInterval(() => (now.value = Date.now()), 500)
  // 从别的步骤返回时不要再跑一遍（大模型归纳要几十秒到十几分钟）
  if (w.summary) {
    ready.value = true
  } else {
    run()
  }
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

const stats = computed(() => w.ai.stats)

const elapsedText = computed(() => {
  const t0 = w.ai.startedAt || now.value
  const sec = Math.max(0, Math.round((now.value - t0) / 1000))
  return `${String(Math.floor(sec / 60)).padStart(2, '0')}:${String(sec % 60).padStart(2, '0')}`
})

const percent = computed(() => {
  const { done, total, phase } = w.ai
  if (!total) return phase === 'rules' ? 4 : 0
  // 预热阶段占模型阶段的 1%，避免进度条一直 0%
  const base = phase === 'warmup' ? 0.5 : done
  return Math.min(100, Math.round((base / total) * 100))
})

const etaText = computed(() => {
  const { done, total } = w.ai
  if (done < 2 || !total) return ''
  const per = (now.value - w.ai.startedAt) / done
  const rest = Math.max(0, ((total - done) * per) / 1000)
  if (rest < 5) return '即将完成'
  return rest > 60 ? `约剩 ${Math.round(rest / 60)} 分钟` : `约剩 ${Math.round(rest)} 秒`
})

const phaseText = computed(() => {
  const p = w.ai.phase
  if (p === 'llm') {
    return `本地大模型正在逐份阅读（${w.ai.done}/${w.ai.total}）· ${w.ai.current}`
  }
  if (p === 'warmup') {
    return `正在把本地大模型加载进显存（本次需复核 ${w.ai.total} 份，首次约 10 秒）…`
  }
  return PHASE_TEXT[p as string] ?? '正在理解你的文件…'
})

const selectedName = computed(() => {
  if (!selected.value) return '全部文件'
  const c = data.categories.find((x) => x.id === selected.value)
  return c?.name ?? '全部文件'
})

const visibleFiles = computed(() => {
  let list = data.files
  if (selected.value) {
    const ids = new Set<string>([selected.value])
    data.categories
      .filter((c) => c.parentId === selected.value)
      .forEach((c) => ids.add(c.id))
    list = list.filter((f) => f.categoryId && ids.has(f.categoryId))
  }
  if (mode.value === 'confidence') {
    list = [...list].sort((a, b) => a.confidence - b.confidence)
  }
  return list
})

const lowCount = computed(
  () => data.files.filter((f) => f.confidence < data.settings.confidenceThreshold).length
)

const KIND_LABEL: Record<string, string> = {
  doc: '文档',
  xls: '表格',
  ppt: '演示',
  pdf: 'PDF',
  image: '图片',
  drawio: '架构图',
  text: '文本',
  code: '代码',
  other: '其他',
}

const byKind = computed(() => {
  const m = new Map<string, number>()
  for (const f of data.files) m.set(f.kind, (m.get(f.kind) ?? 0) + 1)
  return Array.from(m.entries()).map(([k, n]) => ({
    label: KIND_LABEL[k] ?? k,
    count: n,
  }))
})

/** 结果页顶部的引擎分工说明 */
const engineLine = computed(() => {
  const s = stats.value
  if (!s) return ''
  if (!s.llmUsed) {
    return s.fallbackReason ? `本次仅用规则引擎：${s.fallbackReason}` : '本次仅用规则引擎'
  }
  const bits = [`${s.model} 参与判定 ${s.llmDone} 份`]
  if (s.agree) bits.push(`双路一致 ${s.agree}`)
  if (s.filled) bits.push(`模型补判 ${s.filled}`)
  if (s.conflict) bits.push(`分歧待确认 ${s.conflict}`)
  return bits.join(' · ')
})

/** 没有可读正文的文件（图片/扫描件/旧版二进制 Office）——模型看不到更多信息，只能人工 */
const noBodyCount = computed(
  () =>
    data.files.filter((f) => (f.textSummary?.trim().length ?? 0) <= 30 && !f.drawioNodes?.length)
      .length
)

/** 本次送模型的上限说明（避免用户以为模型看了全部文件） */
const capNote = computed(() => {
  const s = stats.value
  if (!s || !s.llmUsed) return ''
  const total = data.files.length
  if (s.llmDone >= total) return ''
  return `共 ${total} 份，本次挑了 ${s.llmDone} 份交给模型，其余保留规则结果`
})

function next() {
  w.goto(4)
}
</script>

<template>
  <div class="step">
    <!-- 归纳进行中 -->
    <div v-if="!ready" class="running">
      <div class="orb">
        <AppIcon name="sparkle" :size="24" />
      </div>
      <h2>AI 正在理解你的文件</h2>
      <p class="phase-line">{{ phaseText }}</p>

      <div class="bar-wrap">
        <div class="bar">
          <div class="bar-fill" :style="{ width: percent + '%' }" />
        </div>
        <div class="bar-meta">
          <span class="pct">{{ percent }}%</span>
          <span class="grow" />
          <span class="muted">{{ elapsedText }}</span>
          <span v-if="etaText" class="muted">{{ etaText }}</span>
        </div>
      </div>

      <!-- 实时判定结果：边跑边出，而不是干等 -->
      <div class="live">
        <header class="live-head">
          <span class="live-title">实时判定</span>
          <span class="muted tiny">
            {{ w.ai.phase === 'llm' ? `已复核 ${w.ai.done} 份` : '规则初判已完成，等待模型复核' }}
          </span>
        </header>
        <div class="live-list">
          <div v-for="e in w.ai.log.slice(0, 7)" :key="e.id" class="live-item">
            <span class="src" :class="'src-' + e.source">{{ SOURCE_LABEL[e.source] }}</span>
            <span class="li-name">{{ e.name }}</span>
            <span class="li-cat">
              {{ e.ws ? (e.dt ? `${e.ws} · ${e.dt}` : e.ws) : '待人工归类' }}
            </span>
            <span class="li-conf">{{ Math.round(e.confidence * 100) }}%</span>
          </div>
          <div v-if="!w.ai.log.length" class="live-empty muted tiny">
            {{
              w.ai.phase === 'rules'
                ? '正在用词典证据快速初判，几毫秒即可扫完全部文件…'
                : '正在挑选需要模型复核的文件…'
            }}
          </div>
        </div>
      </div>

      <button class="btn btn-secondary btn-sm" @click="w.cancelInduction()">
        中止，先看已判完的
      </button>
    </div>

    <!-- 归纳结果 -->
    <template v-else>
      <div class="step-body">
        <div class="head">
          <div>
            <h1>AI 已归纳出你的分类体系</h1>
            <p class="sub">
              共 {{ data.files.length }} 个文件 ·
              {{ data.categories.filter((c) => !c.parentId).length }} 个分类 ·
              <span class="warn-text">{{ lowCount }} 个置信度偏低，会交给你确认</span>
            </p>
            <p v-if="engineLine" class="engine" :class="{ bad: !stats?.llmUsed }">
              <AppIcon :name="stats?.llmUsed ? 'sparkle' : 'info'" :size="12" />
              {{ engineLine }}
            </p>
            <p v-if="capNote" class="engine-note">{{ capNote }}</p>
            <p v-if="noBodyCount" class="engine-note">
              {{ noBodyCount }} 份文件抽不到正文（图片、扫描件或旧版 doc/xls），模型看不到更多信息，已留给你确认
            </p>
          </div>
          <div class="kinds">
            <span v-for="k in byKind" :key="k.label" class="kd">{{ k.label }} {{ k.count }}</span>
          </div>
        </div>

        <div class="grid">
          <div class="panel tree-panel">
            <CategoryTree :tree="data.categoryTree" :selected="selected" @select="selected = $event" />
          </div>

          <div class="panel list-panel">
            <header class="lp-head">
              <span class="lp-title">{{ selectedName }}</span>
              <span class="lp-count">{{ visibleFiles.length }} 个文件</span>
              <div class="grow" />
              <SegmentedControl
                v-model="mode"
                :options="[
                  { label: '全部', value: 'all' },
                  { label: '按置信度', value: 'confidence' },
                ]"
              />
            </header>
            <div class="scroll lp-list">
              <FileListItem v-for="f in visibleFiles" :key="f.id" :file="f" />
            </div>
          </div>
        </div>
      </div>

      <div class="step-foot">
        <button class="btn btn-secondary" @click="run">
          <AppIcon name="refresh" :size="14" />
          重新归纳
        </button>
        <div class="spacer" />
        <span class="muted tiny">{{ lowCount }} 个待确认可稍后处理</span>
        <button class="btn btn-primary" @click="next">
          进入人工微调
          <AppIcon name="arrow-right" :size="14" />
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* ---- running ---- */
.running {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding-bottom: 24px;
}
.orb {
  width: 60px;
  height: 60px;
  border-radius: 19px;
  background: linear-gradient(135deg, #e8f0ff, #dbe8ff);
  color: var(--c-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 8px 28px rgba(0, 122, 255, 0.18);
  animation: float 2.4s ease-in-out infinite;
}
@keyframes float {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-6px);
  }
}
.running h2 {
  font-size: 18px;
  font-weight: 500;
  margin: 4px 0 0;
}
.phase-line {
  font-size: 12.5px;
  color: var(--c-text-2);
  margin: 0;
  max-width: 620px;
  text-align: center;
  line-height: 1.6;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.bar-wrap {
  width: 440px;
  max-width: 70vw;
  margin-top: 4px;
}
.bar {
  height: 6px;
  border-radius: 4px;
  background: rgba(120, 120, 128, 0.14);
  overflow: hidden;
}
.bar-fill {
  height: 100%;
  border-radius: 4px;
  background: var(--c-primary);
  transition: width 0.35s ease;
}
.bar-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 7px;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.pct {
  color: var(--c-primary);
  font-weight: 500;
}
.grow {
  flex: 1;
}

.live {
  width: 560px;
  max-width: 78vw;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  overflow: hidden;
  margin-top: 2px;
}
.live-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 14px;
  border-bottom: 0.5px solid var(--c-divider);
}
.live-title {
  font-size: 12px;
  font-weight: 500;
}
.live-list {
  padding: 4px 0;
  min-height: 168px;
}
.live-item {
  display: grid;
  grid-template-columns: 62px minmax(0, 1fr) 150px 38px;
  align-items: center;
  gap: 8px;
  padding: 5px 14px;
  font-size: 11.5px;
  animation: slidein 0.25s ease;
}
@keyframes slidein {
  from {
    opacity: 0;
    transform: translateY(-3px);
  }
}
.li-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--c-text);
}
.li-cat {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--c-text-2);
  text-align: right;
}
.li-conf {
  text-align: right;
  color: var(--c-text-2);
  font-variant-numeric: tabular-nums;
}
.src {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 4px;
  text-align: center;
  white-space: nowrap;
}
.src-agree {
  color: #0a7d3f;
  background: rgba(52, 199, 89, 0.14);
}
.src-llm {
  color: var(--c-primary);
  background: rgba(0, 122, 255, 0.1);
}
.src-conflict {
  color: #a86700;
  background: rgba(255, 149, 0, 0.16);
}
.src-rule {
  color: var(--c-text-2);
  background: rgba(0, 0, 0, 0.05);
}
.src-none {
  color: var(--c-text-2);
  background: rgba(0, 0, 0, 0.05);
}
.live-empty {
  padding: 12px 14px;
}

/* ---- result ---- */
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
.engine {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 11.5px;
  color: var(--c-primary);
  background: rgba(0, 122, 255, 0.07);
  border-radius: 6px;
  padding: 3px 9px;
  margin: 7px 0 0;
}
.engine.bad {
  color: var(--c-warning-deep, #a86700);
  background: rgba(255, 149, 0, 0.1);
}
.engine-note {
  font-size: 11px;
  color: var(--c-text-2);
  margin: 5px 0 0;
}
.warn-text {
  color: var(--c-warning-deep);
}
.kinds {
  display: flex;
  gap: 5px;
  flex-wrap: wrap;
  justify-content: flex-end;
  max-width: 300px;
}
.kd {
  font-size: 11px;
  color: var(--c-text-2);
  background: rgba(0, 0, 0, 0.04);
  border-radius: 6px;
  padding: 3px 8px;
}

.grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 225px minmax(0, 1fr);
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

.lp-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 11px 16px;
  border-bottom: 0.5px solid var(--c-border);
  flex: 0 0 auto;
}
.lp-title {
  font-size: 13px;
  font-weight: 500;
}
.lp-count {
  font-size: 11.5px;
  color: var(--c-text-2);
}
.lp-list {
  flex: 1;
  min-height: 0;
}

.step-body {
  gap: 12px;
}
</style>
