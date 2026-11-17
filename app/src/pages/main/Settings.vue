<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import { useDataStore } from '../../stores/data'
import { useWizardStore } from '../../stores/wizard'
import { listModels, DEFAULT_BASE_URL, type OllamaModelInfo } from '../../core/llm'

const data = useDataStore()
const w = useWizardStore()
const router = useRouter()
const saved = ref(false)

const s = data.settings

const llmState = ref<'idle' | 'checking' | 'ok' | 'fail'>('idle')
const llmModels = ref<OllamaModelInfo[]>([])
const llmMsg = ref('')

/** 探测本地模型服务，顺便刷新可用模型列表 */
async function probe() {
  llmState.value = 'checking'
  const r = await listModels(s.llmBaseUrl || DEFAULT_BASE_URL)
  if (r.ok) {
    llmState.value = 'ok'
    llmModels.value = r.models
    llmMsg.value = `已连接 · 本地 ${r.models.length} 个模型`
    if (r.models.length && !r.models.some((m) => m.name === s.llmModel)) {
      s.llmModel = r.models[0].name
    }
  } else {
    llmState.value = 'fail'
    llmModels.value = []
    llmMsg.value = `未连接：${r.error ?? '请先启动 Ollama'}`
  }
}

onMounted(() => {
  if (s.llmEnabled) probe()
})

function thresholdText() {
  return `${Math.round(s.confidenceThreshold * 100)}%`
}

async function save() {
  await data.saveSettings()
  saved.value = true
  setTimeout(() => (saved.value = false), 1800)
}

function restart() {
  data.files = []
  data.categories = []
  data.records = []
  data.initialized = false
  w.reset()
  router.push('/wizard')
}
</script>

<template>
  <div class="page scroll">
    <header class="top">
      <div>
        <h1 class="page-title">设置</h1>
        <p class="page-sub">影响扫描、分类与自动整理的行为</p>
      </div>
      <button class="btn btn-primary btn-sm" @click="save">
        <AppIcon v-if="saved" name="check" :size="13" />
        {{ saved ? '已保存' : '保存设置' }}
      </button>
    </header>

    <section class="group">
      <h2 class="g-title">归档位置</h2>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">归档根目录</div>
          <div class="ri-desc">所有迁移结果都在这个位置，扫描时自动排除自身</div>
        </div>
        <code class="path">{{ s.rootPath }}</code>
      </div>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">单文件大小上限</div>
          <div class="ri-desc">超过该大小的文件跳过，不扫描不迁移</div>
        </div>
        <div class="num">
          <input v-model.number="s.maxFileSizeMB" type="number" class="field tiny-field" />
          <span class="unit">MB</span>
        </div>
      </div>
    </section>

    <section class="group">
      <h2 class="g-title">AI 分类</h2>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">置信度阈值</div>
          <div class="ri-desc">
            低于 {{ thresholdText() }} 的文件不自动归档，进入待确认队列交给你
          </div>
        </div>
        <div class="slider-wrap">
          <input
            v-model.number="s.confidenceThreshold"
            type="range"
            min="0.5"
            max="0.95"
            step="0.05"
            class="slider"
          />
          <span class="sval">{{ thresholdText() }}</span>
        </div>
      </div>

      <div class="row-item col">
        <div class="ri-main">
          <div class="ri-label">
            本地大模型（Ollama）
            <span class="chip" :class="'st-' + llmState">
              {{ llmState === 'ok' ? '已连接' : llmState === 'fail' ? '未连接' : llmState === 'checking' ? '检测中' : '未检测' }}
            </span>
          </div>
          <div class="ri-desc">
            开启后，规则引擎先扫一遍，再把没把握的文件交给本地模型复核。规则与模型判断一致的文件自动归档，
            两者不一致的一律进待确认，模型补判的文件会标注来源。
          </div>
        </div>

        <div class="llm-form">
          <div class="lf-row">
            <label class="lf-label">启用大模型</label>
            <button
              class="switch"
              :class="{ on: s.llmEnabled }"
              @click="s.llmEnabled = !s.llmEnabled; s.llmEnabled && llmState === 'idle' && probe()"
            >
              <span class="knob" />
            </button>
          </div>

          <div class="lf-row">
            <label class="lf-label">服务地址</label>
            <div class="lf-inline">
              <input v-model="s.llmBaseUrl" class="field" spellcheck="false" />
              <button class="btn btn-secondary btn-sm" :disabled="llmState === 'checking'" @click="probe">
                {{ llmState === 'checking' ? '检测中' : '检测连接' }}
              </button>
            </div>
          </div>

          <div class="lf-row">
            <label class="lf-label">模型</label>
            <div class="lf-inline">
              <select v-if="llmModels.length" v-model="s.llmModel" class="field">
                <option v-for="m in llmModels" :key="m.name" :value="m.name">
                  {{ m.name }}（{{ m.sizeGB }}GB{{ m.vision ? ' · 支持看图' : '' }}）
                </option>
              </select>
              <input v-else v-model="s.llmModel" class="field" spellcheck="false" placeholder="qwen2.5:7b" />
            </div>
          </div>

          <div class="lf-row">
            <label class="lf-label">送模型范围</label>
            <select v-model="s.llmStrategy" class="field">
              <option value="smart">智能（只复核规则没把握的，推荐）</option>
              <option value="all">全部文件双路交叉验证（最准，最慢）</option>
            </select>
          </div>

          <div class="lf-row">
            <label class="lf-label">并发数</label>
            <div class="lf-inline">
              <input v-model.number="s.llmConcurrency" type="number" min="1" max="4" class="field tiny-field" />
              <span class="unit">显存有限，建议 2。实测 Ollama 默认串行推理，调高几乎不提速</span>
            </div>
          </div>

          <div class="lf-row">
            <label class="lf-label">单次上限</label>
            <div class="lf-inline">
              <input v-model.number="s.llmMaxFiles" type="number" min="0" step="50" class="field tiny-field" />
              <span class="unit">送模型的最大文件数，0 = 不限。默认 60 份≈3 分钟，超额部分保留规则结果</span>
            </div>
          </div>

          <p v-if="llmMsg" class="llm-msg" :class="llmState">
            {{ llmMsg }}
            <template v-if="llmState === 'fail'">
              —— 在命令行执行 <code>ollama serve</code> 即可，或在开始菜单打开 Ollama
            </template>
          </p>
          <p class="llm-msg hint">
            参考速度（RTX 4060 Laptop 实测）：qwen2.5:7b 约 3 秒/份，qwen3.5:9b 约 4.5 秒/份
          </p>
        </div>
      </div>
    </section>

    <section class="group">
      <h2 class="g-title">自动整理</h2>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">空闲自动整理</div>
          <div class="ri-desc">电脑空闲时增量扫描新文件并自动归位，只在空闲与接电源时运行</div>
        </div>
        <button
          class="switch"
          :class="{ on: s.autoOrganizeEnabled }"
          @click="s.autoOrganizeEnabled = !s.autoOrganizeEnabled"
        >
          <span class="knob" />
        </button>
      </div>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">空闲判定时长</div>
          <div class="ri-desc">连续无操作超过此时长才触发整理</div>
        </div>
        <div class="num">
          <input
            v-model.number="s.idleThresholdSec"
            type="number"
            class="field tiny-field"
            step="60"
          />
          <span class="unit">秒</span>
        </div>
      </div>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">仅接电源时运行</div>
          <div class="ri-desc">使用电池时不整理，避免额外耗电</div>
        </div>
        <button
          class="switch"
          :class="{ on: s.requirePower }"
          @click="s.requirePower = !s.requirePower"
        >
          <span class="knob" />
        </button>
      </div>
    </section>

    <section class="group danger">
      <h2 class="g-title">重新初始化</h2>
      <div class="row-item">
        <div class="ri-main">
          <div class="ri-label">清空索引并重新运行向导</div>
          <div class="ri-desc">只清空本程序的索引数据，不会删除你的任何文件</div>
        </div>
        <button class="btn btn-secondary btn-sm" @click="restart">重新初始化</button>
      </div>
    </section>
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
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  flex: 0 0 auto;
}
.group {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  overflow: hidden;
  flex: 0 0 auto;
}
.g-title {
  font-size: 12px;
  font-weight: 500;
  color: var(--c-text-2);
  margin: 0;
  padding: 12px 18px 10px;
  border-bottom: 0.5px solid var(--c-border);
  background: rgba(0, 0, 0, 0.012);
}
.row-item {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 13px 18px;
  border-bottom: 0.5px solid var(--c-divider);
}
.row-item:last-child {
  border-bottom: none;
}
.row-item.col {
  flex-direction: column;
  align-items: stretch;
}
.ri-main {
  flex: 1;
  min-width: 0;
}
.ri-label {
  font-size: 13px;
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 6px;
}
.ri-desc {
  font-size: 11.5px;
  color: var(--c-text-2);
  margin-top: 2px;
  line-height: 1.5;
}
.path {
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 11.5px;
  color: var(--c-text-2);
  background: var(--c-field);
  padding: 5px 10px;
  border-radius: 7px;
}
.num {
  display: flex;
  align-items: center;
  gap: 6px;
}
.tiny-field {
  width: 96px;
  padding: 7px 10px;
  text-align: right;
}
.unit {
  font-size: 12px;
  color: var(--c-text-2);
}
.slider-wrap {
  display: flex;
  align-items: center;
  gap: 10px;
}
.slider {
  width: 180px;
  accent-color: var(--c-primary);
}
.sval {
  font-size: 12px;
  color: var(--c-text);
  font-variant-numeric: tabular-nums;
  min-width: 36px;
  text-align: right;
}
.llm-form {
  display: flex;
  flex-direction: column;
  gap: 9px;
  margin-top: 12px;
  padding: 12px 14px;
  background: var(--c-field);
  border-radius: 10px;
}
.lf-row {
  display: grid;
  grid-template-columns: 96px minmax(0, 1fr);
  align-items: center;
  gap: 10px;
}
.lf-label {
  font-size: 12px;
  color: var(--c-text-2);
}
.lf-row:first-child {
  grid-template-columns: 96px minmax(0, 1fr);
}
.lf-row .switch {
  justify-self: start;
}
.lf-inline {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.lf-inline .field {
  min-width: 0;
}
.llm-msg {
  font-size: 11px;
  color: var(--c-text-2);
  margin: 2px 0 0;
  line-height: 1.6;
}
.llm-msg.ok {
  color: #0a7d3f;
}
.llm-msg.fail {
  color: var(--c-danger);
}
.llm-msg.hint {
  color: var(--c-text-3, var(--c-text-2));
  opacity: 0.8;
}
.llm-msg code {
  font-family: ui-monospace, Menlo, Consolas, monospace;
  background: rgba(0, 0, 0, 0.05);
  padding: 1px 4px;
  border-radius: 4px;
}
.chip {
  font-size: 10px;
  font-weight: 500;
  color: var(--c-text-2);
  background: rgba(142, 142, 147, 0.14);
  border-radius: 5px;
  padding: 1px 6px;
}
.chip.st-ok {
  color: #0a7d3f;
  background: rgba(52, 199, 89, 0.16);
}
.chip.st-fail {
  color: var(--c-danger);
  background: rgba(255, 59, 48, 0.12);
}
.switch {
  width: 42px;
  height: 25px;
  border-radius: 20px;
  background: rgba(120, 120, 128, 0.16);
  position: relative;
  transition: background 0.2s ease;
  flex: 0 0 42px;
}
.switch.on {
  background: var(--c-success);
}
.knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 21px;
  height: 21px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  transition: transform 0.2s cubic-bezier(0.4, 0, 0.2, 1);
}
.switch.on .knob {
  transform: translateX(17px);
}
.danger .g-title {
  color: var(--c-danger);
}
</style>
