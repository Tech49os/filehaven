<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const ready = ref(false)
/** 有存档时先让用户选：沿用（秒出、类目不变）还是重新生成（可能更贴合这批文件） */
const asking = ref(false)
const reused = ref(false)
const hint = ref('')

const topicCount = computed(() => w.proposal?.topics.length ?? 0)
const ungrouped = computed(() => w.proposal?.ungroupedIds.length ?? 0)
const fileCount = computed(() => w.scannedCount)
/** 分出了几个大类（归档第一层目录） */
const domainCount = computed(
  () => new Set((w.proposal?.topics ?? []).map((t) => t.domain).filter(Boolean)).size
)

const savedInfo = computed(() => {
  const s = w.savedPlan
  if (!s) return null
  const d = new Date(s.savedAt)
  const pad = (n: number) => String(n).padStart(2, '0')
  return {
    count: s.topics.length,
    when: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`,
    root: s.root,
  }
})

async function generate() {
  asking.value = false
  reused.value = false
  hint.value = ''
  ready.value = false
  try {
    await w.runProposal()
  } finally {
    ready.value = true
  }
}

function reuse() {
  asking.value = false
  const ok = w.useSavedPlan()
  ready.value = true
  if (ok) {
    reused.value = true
  } else {
    hint.value = '上次的分类名在这批文件里一个都没认领到——可能是换了一批完全不同的文件。重新生成吧。'
    ready.value = true
  }
}

async function forget() {
  await w.forgetSavedPlan()
  await generate()
}

onMounted(async () => {
  // 从确认页返回时不要重跑（会丢掉用户已经改好的方案）
  if (w.proposal) {
    ready.value = true
    return
  }
  if (w.savedPlan) {
    asking.value = true
    ready.value = true
    return
  }
  await generate()
})
</script>

<template>
  <div class="ind">
    <div class="ind-box">
      <!-- 有存档：先问一句，别默默跑掉几十秒 -->
      <template v-if="asking">
        <div class="ind-title">上次的分类方案还在</div>
        <div class="ind-sub">
          上次保存于 {{ savedInfo!.when }}，共 {{ savedInfo!.count }} 个分类。<br />
          沿用会直接按这些分类名去认领这次的文件，几秒出结果，类目和你上次调好的完全一致；
          重新生成则会按这批文件的命名规律重抽一遍（模型可用时要等半分钟）。
        </div>
        <div class="ind-actions">
          <button class="ind-btn primary" @click="reuse">沿用上次方案</button>
          <button class="ind-btn" @click="generate">重新生成</button>
        </div>
        <button class="ind-link" @click="forget">不再使用上次方案</button>
      </template>

      <template v-else-if="!ready">
        <div class="ind-spin"></div>
        <div class="ind-title">正在归纳这次要处理哪些事…</div>
        <div class="ind-sub">
          {{ w.ai.current || '正在从文件名里抽取事项…' }}<br />
          先用规则从文件名里抽出反复出现的专名，再让本地模型做两件小事：哪些词只是人名/单位名、
          这批文件主要在管哪些方向（决定第一层目录）。同一份文档存了多个版本的，统计时只算最新那份，
          免得「存了七个版本的 PPT」霸占类目。模型较慢时前后要等一两分钟；
          没装或没启动会自动跳过，只按名字规律分。
        </div>
      </template>

      <template v-else-if="hint">
        <div class="ind-title warn">没能沿用上次方案</div>
        <div class="ind-sub">{{ hint }}</div>
        <button class="ind-btn primary" @click="generate">重新生成</button>
      </template>

      <template v-else-if="w.error">
        <div class="ind-title warn">没能生成分类方案</div>
        <div class="ind-sub">{{ w.error }}</div>
        <button class="ind-btn" @click="generate">重试</button>
      </template>

      <template v-else>
        <div class="ind-done">
          <div class="ind-num">{{ topicCount }}</div>
          <div class="ind-title">个事项</div>
        </div>
        <div class="ind-sub">
          <template v-if="reused">已沿用上次方案，</template>
          <template v-else>从 {{ fileCount }} 份文件里归纳出</template>
          {{ topicCount }} 个事项<template v-if="domainCount">、{{ domainCount }} 个大类</template>，{{ ungrouped }} 份没能归类。
          下一步可以改名字、删掉不需要的、把重复的合并。
        </div>
        <button class="ind-btn primary" @click="w.next()">查看并确认分类方案</button>
        <button v-if="reused" class="ind-link" @click="generate">
          这批文件变化较大，改为重新生成
        </button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.ind {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}
.ind-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  max-width: 520px;
  text-align: center;
}
.ind-spin {
  width: 28px;
  height: 28px;
  border: 2px solid var(--c-border);
  border-top-color: var(--c-primary);
  border-radius: 50%;
  animation: spin 0.9s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
.ind-done {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.ind-num {
  font-size: 40px;
  font-weight: 500;
  color: var(--c-primary);
  line-height: 1;
}
.ind-title {
  font-size: 16px;
  font-weight: 500;
  color: var(--c-text);
}
.ind-title.warn {
  color: var(--c-warning);
}
.ind-sub {
  font-size: 13px;
  line-height: 1.8;
  color: var(--c-text-2);
}
.ind-actions {
  display: flex;
  gap: 10px;
  margin-top: 4px;
}
.ind-btn {
  margin-top: 8px;
  font-size: 13px;
  padding: 9px 22px;
  border-radius: 8px;
  cursor: pointer;
  border: 0.5px solid var(--c-border);
  background: var(--c-surface);
  color: var(--c-text);
}
.ind-actions .ind-btn {
  margin-top: 0;
}
.ind-btn.primary {
  background: var(--c-primary);
  border-color: var(--c-primary);
  color: #fff;
}
.ind-link {
  margin-top: 2px;
  font-size: 12px;
  color: var(--c-text-3);
  background: none;
  border: none;
  cursor: pointer;
  padding: 4px 6px;
}
.ind-link:hover {
  color: var(--c-text-2);
  text-decoration: underline;
}
</style>
