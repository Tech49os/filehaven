<script setup lang="ts">
/**
 * 目录方案页（T6.7）
 *
 * 三件事必须做到：
 * 1. 等待期只报「阶段 + 已等多少秒」，不报假百分比；
 * 2. 双列并排，每项能展开看依据（为什么建议这个目录）；
 * 3. 模型不可用 / 输出不可用时整块降级为手动输入，说清原因，不让用户卡住。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import NewDirInput from '../../components/NewDirInput.vue'
import * as ipc from '../../core/ipc'
import { onSuggestStage } from '../../core/events'
import { ERR, humanize, toCmdError } from '../../core/cmdError'
import { useAppStore } from '../../stores/app'
import { useDirsStore } from '../../stores/dirs'
import type { SuggestedDir, SuggestStage } from '../../core/types'
import type { UnlistenFn } from '@tauri-apps/api/event'

const router = useRouter()
const app = useAppStore()
const dirs = useDirsStore()

const loading = ref(true)
const stage = ref<SuggestStage | null>(null)
const elapsedSec = ref(0)
const columns = ref<{ key: 'A' | 'B'; title: string; dirs: SuggestedDir[] }[]>([])
const selected = ref<Set<string>>(new Set())
const expanded = ref<Set<string>>(new Set())
const manual = ref(false)
const degradeReason = ref<string | null>(null)
const err = ref<string | null>(null)
const busy = ref(false)
const manualAdded = ref<string[]>([])

let tick: ReturnType<typeof setInterval> | null = null
let unlisten: UnlistenFn | null = null
let cancelled = false

function startHeartbeat() {
  elapsedSec.value = 0
  if (tick) clearInterval(tick)
  tick = setInterval(() => {
    if (!loading.value) return
    elapsedSec.value += 1
  }, 1000)
}

function stopHeartbeat() {
  if (tick) clearInterval(tick)
  tick = null
}

const stageText = computed(() => {
  const s = stage.value
  const sec = s ? Math.round(s.elapsedMs / 1000) : elapsedSec.value
  const head =
    s?.stage === 'validating'
      ? '正在校验方案'
      : s?.stage === 'sampling'
        ? '正在抽样文件名'
        : '正在归纳管理维度'
  return `${head} · 已等 ${sec} 秒`
})

async function load(force: boolean) {
  if (cancelled) return
  loading.value = true
  err.value = null
  degradeReason.value = null
  selected.value = new Set()
  expanded.value = new Set()
  startHeartbeat()
  try {
    const s = await ipc.suggestDirs(force)
    if (cancelled) return
    columns.value = [
      { key: 'A', title: '方案 A', dirs: s.optionA ?? [] },
      { key: 'B', title: '方案 B', dirs: s.optionB ?? [] },
    ].filter((c) => c.dirs.length > 0) as typeof columns.value
    if (columns.value.length === 0) {
      manual.value = true
      degradeReason.value = '模型这次没给出可用方案，先自己写几个目录'
    }
  } catch (e) {
    if (cancelled) return
    const ce = toCmdError(e)
    // 模型类失败一律降级，不把配置问题甩给用户
    manual.value = true
    if (ce.code === ERR.UNKNOWN_CMD) {
      degradeReason.value = '目录建议还没接入（下一步会补上），现在先手动建几个目录'
    } else if (
      ce.code === ERR.MODEL_DOWN ||
      ce.code === ERR.MODEL_TIMEOUT ||
      ce.code === ERR.MODEL_BAD_OUTPUT
    ) {
      degradeReason.value = `${humanize(ce)}，手动写几个也一样能开始`
    } else {
      degradeReason.value = humanize(ce)
    }
  } finally {
    if (!cancelled) {
      loading.value = false
      stopHeartbeat()
    }
  }
}

function toggle(name: string) {
  const next = new Set(selected.value)
  if (next.has(name)) next.delete(name)
  else next.add(name)
  selected.value = next
}

function toggleBasis(name: string) {
  const next = new Set(expanded.value)
  if (next.has(name)) next.delete(name)
  else next.add(name)
  expanded.value = next
}

const selectedNames = computed(() => [...selected.value])
const canAccept = computed(() => selectedNames.value.length > 0 && !busy.value)

async function accept() {
  if (!canAccept.value) return
  busy.value = true
  err.value = null
  try {
    await ipc.acceptDirs(selectedNames.value)
    await dirs.load()
    await app.refreshBoot()
    void router.replace('/workspace')
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    busy.value = false
  }
}

async function onManualConfirm(input: string) {
  err.value = null
  try {
    const d = await ipc.acceptDirs([input])
    manualAdded.value.push(...d.map((x) => x.relPath))
    await dirs.load()
    await app.refreshBoot()
  } catch (e) {
    err.value = humanize(toCmdError(e))
  }
}

function skip() {
  void router.replace('/workspace')
}

onMounted(async () => {
  unlisten = await onSuggestStage((p) => {
    stage.value = p
  })
  if (dirs.count === 0) await dirs.load()
  await load(false)
})

onUnmounted(() => {
  // 取消：卸载即视为用户不看了（后端没有真正的中断接口，这里只丢弃结果）
  cancelled = true
  stopHeartbeat()
  unlisten?.()
})
</script>

<template>
  <div class="wrap">
    <header class="hd">
      <h1>挑一套目录结构</h1>
      <p class="sub">
        下面两套是按你现有文件名归纳出来的。挑中意的那套勾上（可以只勾几个），也可以自己写。
      </p>
    </header>

    <p v-if="err" class="err" role="alert">{{ err }}</p>

    <!-- 等待：只报阶段与已等时间，不报假百分比 -->
    <section v-if="loading" class="waiting">
      <div class="pulse"><i /><i /><i /></div>
      <p class="wtext">{{ stageText }}</p>
      <p v-if="stage?.note" class="wnote">{{ stage.note }}</p>
      <p class="wnote">首次运行要加载模型，慢一点正常。你也可以不等，直接自己写。</p>
      <button class="btn" @click="manual = true; loading = false">不等了，我自己写</button>
    </section>

    <!-- 双列方案 -->
    <section v-else-if="!manual" class="cols">
      <div v-for="c in columns" :key="c.key" class="col">
        <h2>{{ c.title }}</h2>
        <ul class="dlist">
          <li v-for="d in c.dirs" :key="`${c.key}-${d.name}`" class="drow">
            <label class="dmain">
              <input
                type="checkbox"
                :checked="selected.has(d.name)"
                @change="toggle(d.name)"
              />
              <span class="dname">{{ d.name }}</span>
              <span class="dcount">{{ d.fileCount }} 份</span>
              <button
                class="basis-btn"
                :aria-expanded="expanded.has(d.name)"
                @click.prevent="toggleBasis(d.name)"
              >
                依据
                <AppIcon :name="expanded.has(d.name) ? 'chevron-down' : 'chevron-right'" :size="12" />
              </button>
            </label>
            <p v-if="expanded.has(d.name)" class="basis">{{ d.basis }}</p>
          </li>
          <li v-if="c.dirs.length === 0" class="empty">这套是空的</li>
        </ul>
      </div>
    </section>

    <!-- 降级：手动输入 -->
    <section v-else class="manual">
      <p v-if="degradeReason" class="degrade">
        <AppIcon name="info" :size="14" />
        <span>{{ degradeReason }}</span>
      </p>
      <p class="lead">写几个你会用到的分类，多级用 <span class="mono">/</span> 分隔，比如「安全检查/年度检查」。</p>
      <NewDirInput placeholder="例如：合同/采购" @confirm="onManualConfirm" @cancel="skip" />
      <ul v-if="manualAdded.length" class="added">
        <li v-for="p in manualAdded" :key="p">
          <AppIcon name="check" :size="12" /><span class="mono">{{ p }}</span>
        </li>
      </ul>
    </section>

    <footer class="foot">
      <button v-if="!manual" class="btn primary" :disabled="!canAccept" @click="accept">
        采纳并建目录{{ selectedNames.length ? `（${selectedNames.length}）` : '' }}
      </button>
      <button v-if="!manual" class="btn" :disabled="loading || busy" @click="load(true)">换一批</button>
      <button v-if="!manual" class="btn" @click="manual = true">自己写</button>
      <button v-else class="btn primary" :disabled="manualAdded.length === 0" @click="skip">
        就用这些，去整理
      </button>
      <button class="btn ghost" @click="skip">先跳过</button>
    </footer>
  </div>
</template>

<style scoped>
.wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 40px 56px 84px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  background: var(--c-bg);
  position: relative;
}

.hd h1 {
  margin: 0;
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.2px;
}
.sub {
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--c-text-2);
}

.waiting {
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  padding: 28px 22px;
  max-width: 640px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}
.pulse {
  display: flex;
  gap: 6px;
}
.pulse i {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--c-primary);
  animation: blink 1.2s infinite ease-in-out;
}
.pulse i:nth-child(2) {
  animation-delay: 0.2s;
}
.pulse i:nth-child(3) {
  animation-delay: 0.4s;
}
@keyframes blink {
  0%,
  100% {
    opacity: 0.25;
    transform: translateY(0);
  }
  50% {
    opacity: 1;
    transform: translateY(-2px);
  }
}
.wtext {
  margin: 4px 0 0;
  font-size: 13px;
  font-weight: 500;
}
.wnote {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
  text-align: center;
  max-width: 440px;
  line-height: 1.7;
}

.cols {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
  align-items: start;
}

.col {
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  padding: 14px 16px 8px;
}
.col h2 {
  margin: 0 0 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--c-text-2);
}

.dlist {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.drow {
  border-radius: var(--r-item);
  padding: 2px 2px 2px 0;
}
.drow:hover {
  background: var(--c-hover);
}

.dmain {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 6px;
  font-size: 13px;
  cursor: default;
}

.dname {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dcount {
  font-size: 11px;
  color: var(--c-text-3);
}

.basis-btn {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 11px;
  padding: 2px 4px;
  border-radius: 6px;
}
.basis-btn:hover {
  background: var(--c-primary-bg);
}

.basis {
  margin: 0 6px 8px 30px;
  font-size: 12px;
  color: var(--c-text-2);
  line-height: 1.65;
  padding-left: 8px;
  border-left: 2px solid var(--c-border-strong);
}

.empty {
  padding: 10px 6px;
  font-size: 12px;
  color: var(--c-text-3);
}

.manual {
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  padding: 18px 20px;
  max-width: 680px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.degrade {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin: 0;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
  font-size: 12px;
  line-height: 1.6;
}

.lead {
  margin: 0;
  font-size: 13px;
  color: var(--c-text-2);
}

.added {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.added li {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  border-radius: var(--r-pill);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  font-size: 12px;
}

.foot {
  position: sticky;
  bottom: 0;
  margin-top: auto;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 0 4px;
  background: linear-gradient(to top, var(--c-bg) 72%, transparent);
}

.btn {
  border: 0;
  border-radius: var(--r-btn);
  padding: 8px 14px;
  font-size: 13px;
  background: var(--c-field);
  color: var(--c-text);
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn.ghost {
  margin-left: auto;
  background: transparent;
  color: var(--c-text-2);
}
.btn:disabled {
  opacity: 0.5;
}

.err {
  margin: 0;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-danger-bg);
  color: var(--c-danger);
  font-size: 12px;
}

.mono {
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 12px;
}
</style>
