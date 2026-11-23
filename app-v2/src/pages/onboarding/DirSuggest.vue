<script setup lang="ts">
/**
 * 目录方案页（T6.7）
 *
 * 四件事必须做到：
 * 1. 等待期只报「阶段 + 已等多少秒」，不报假百分比；
 * 2. 双列并排，每项能展开看依据（为什么建议这个目录）；
 * 3. 模型不可用 / 输出不可用时整块降级为手动输入，说清原因，不让用户卡住；
 * 4. **用户能把自己的想法说出来，也能直接改方案**（2026-11-20 实测反馈）。
 *
 * 第 4 条是这轮新增的：之前只有「换一批」——换一批样本文件重跑同一套逻辑，
 * 用户看着两套都差一点却什么也做不了，只能一遍遍重生成。现在两条路：
 * - 上面一个输入框，写了什么就带进 prompt（`suggest_dirs` 的 hint）；
 * - 方案里每个目录能改名、能删、能自己加一条，采纳时传的是改过的名字。
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
import type { SuggestStage } from '../../core/types'
import type { UnlistenFn } from '@tauri-apps/api/event'

/** 方案里的一行。`key` 是稳定标识：改名之后勾选状态不能丢 */
interface EditDir {
  key: string
  name: string
  basis: string
  fileCount: number
  /** 名字校验不过时的原因（就地显示，别等采纳才整批失败） */
  error: string | null
}

interface Column {
  key: 'A' | 'B'
  title: string
  dirs: EditDir[]
}

const router = useRouter()
const app = useAppStore()
const dirs = useDirsStore()

const loading = ref(true)
const stage = ref<SuggestStage | null>(null)
const elapsedSec = ref(0)
const columns = ref<Column[]>([])
const selected = ref<Set<string>>(new Set())
const expanded = ref<Set<string>>(new Set())
const manual = ref(false)
const degradeReason = ref<string | null>(null)
const err = ref<string | null>(null)
const busy = ref(false)
const manualAdded = ref<string[]>([])

/** 用户写的补充要求 */
const hint = ref('')
/** 正在改名的那一行 */
const editingKey = ref<string | null>(null)
const draft = ref('')
/** 自己加的目录用递增序号做 key（不必引入 uuid） */
let addedSeq = 0

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

/**
 * 要方案。
 *
 * `force` = 换一批（换样本）。`hint` 每次都带上：用户写了要求之后按「换一批」
 * 却退回老结果，是最让人莫名其妙的一种行为。
 */
async function load(force: boolean) {
  if (cancelled) return
  loading.value = true
  manual.value = false
  err.value = null
  degradeReason.value = null
  selected.value = new Set()
  expanded.value = new Set()
  editingKey.value = null
  startHeartbeat()
  try {
    const s = await ipc.suggestDirs(force, hint.value.trim())
    if (cancelled) return
    // 列标题带上维度名：「方案 A · 按事项」才看得出两套差在哪
    const title = (base: string, dim?: string) => {
      const d = (dim ?? '').trim()
      return d ? `${base} · ${d}` : base
    }
    const build = (key: 'A' | 'B', list: { name: string; basis: string; fileCount: number }[]) =>
      list.map((d, i) => ({
        key: `${key}-${i}`,
        name: d.name,
        basis: d.basis,
        fileCount: d.fileCount,
        error: null,
      }))
    columns.value = [
      { key: 'A' as const, title: title('方案 A', s.dimensionA), dirs: build('A', s.optionA ?? []) },
      { key: 'B' as const, title: title('方案 B', s.dimensionB), dirs: build('B', s.optionB ?? []) },
    ].filter((c) => c.dirs.length > 0) as Column[]
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
    } else if (ce.code === ERR.MODEL_BAD_OUTPUT) {
      // 门槛没过的实数原因多半是样本太少：每个目录要 2 份支撑，十来份文件凑不出来
      degradeReason.value = `${humanize(ce)}（每个目录至少要 2 份文件支撑，样本少时容易这样）。可以补一句要求再试，或自己写`
    } else if (ce.code === ERR.MODEL_DOWN || ce.code === ERR.MODEL_TIMEOUT) {
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

function toggle(key: string) {
  const next = new Set(selected.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  selected.value = next
}

function toggleBasis(key: string) {
  const next = new Set(expanded.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  expanded.value = next
}

// ---------------------------------------------------------------- 就地改

function startEdit(d: EditDir) {
  editingKey.value = d.key
  draft.value = d.name
}

function cancelEdit() {
  editingKey.value = null
  draft.value = ''
}

/** 提交改名：先过一遍后端的目录名校验，不过就把原因挂在那一行上 */
async function commitEdit() {
  const key = editingKey.value
  if (!key) return
  const d = allDirs().find((x) => x.key === key)
  const name = draft.value.trim()
  editingKey.value = null
  if (!d) return
  if (!name) {
    d.error = '名字不能为空'
    return
  }
  try {
    const pv = await ipc.previewDirName(name)
    if (!pv.valid) {
      d.name = name
      d.error = pv.reason ?? '这个名字不能当目录名'
      return
    }
    d.name = pv.cleaned || name
    d.error = null
  } catch (e) {
    d.error = toCmdError(e).message
  }
}

function allDirs(): EditDir[] {
  return columns.value.flatMap((c) => c.dirs)
}

function removeDir(col: Column, d: EditDir) {
  col.dirs = col.dirs.filter((x) => x.key !== d.key)
  const next = new Set(selected.value)
  next.delete(d.key)
  selected.value = next
  if (expanded.value.has(d.key)) {
    const ex = new Set(expanded.value)
    ex.delete(d.key)
    expanded.value = ex
  }
}

/** 自己加一条：直接进编辑态，空行摆着等人写 */
function addDir(col: Column) {
  addedSeq += 1
  const d: EditDir = {
    key: `${col.key}-add-${addedSeq}`,
    name: '',
    basis: '你自己加的',
    fileCount: 0,
    error: null,
  }
  col.dirs = [...col.dirs, d]
  startEdit(d)
}

// ---------------------------------------------------------------- 采纳

/** 勾中的目录名（按列顺序，改名后取的是新名字） */
const selectedNames = computed(() => {
  const out: string[] = []
  for (const c of columns.value) {
    for (const d of c.dirs) {
      if (selected.value.has(d.key) && d.name.trim()) out.push(d.name.trim())
    }
  }
  return out
})

const hasError = computed(() => allDirs().some((d) => !!d.error))
const canAccept = computed(() => selectedNames.value.length > 0 && !busy.value && !hasError.value)

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

/** 输入框里 Ctrl/⌘+Enter 直接提交要求（手不用离开键盘） */
function onHintKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
    e.preventDefault()
    if (!loading.value && !busy.value) void load(false)
  }
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

    <!-- 补充要求：这是「把想法告诉模型」的唯一入口。
         降级（模型这次没给出可用方案）时也留着 —— 否则用户只能被迫手写，
         连「换个说法再试一次」都做不到。 -->
    <section class="ask">
      <label class="ask-label" for="dir-hint">
        想让方案更贴合你？直接说
        <span class="ask-dim">（模型只看得到文件名，你越具体它越准）</span>
      </label>
      <div class="ask-row">
        <textarea
          id="dir-hint"
          v-model="hint"
          class="hint"
          rows="2"
          maxlength="200"
          placeholder="例如：我们按客户分，主要做招投标和人事培训；别用「项目管理」这种大词"
          @keydown="onHintKey"
        />
        <button class="btn primary" :disabled="loading || busy" @click="load(false)">
          按我说的再来一版
        </button>
      </div>
    </section>

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
          <li v-for="d in c.dirs" :key="d.key" class="drow">
            <label class="dmain">
              <input
                type="checkbox"
                :checked="selected.has(d.key)"
                :disabled="!!d.error"
                @change="toggle(d.key)"
              />
              <input
                v-if="editingKey === d.key"
                v-model="draft"
                class="dname-input"
                placeholder="写个目录名"
                @keydown.enter.stop.prevent="commitEdit"
                @keydown.esc.stop.prevent="cancelEdit"
                @blur="commitEdit"
              />
              <template v-else>
                <!-- 点名字就能改；改名比重新生成一遍快得多 -->
                <span class="dname" :class="{ bad: !!d.error }" @click.prevent="startEdit(d)">
                  {{ d.name || '（点这里写名字）' }}
                </span>
                <span v-if="d.fileCount > 0" class="dcount">样本 {{ d.fileCount }} 份</span>
                <button
                  class="basis-btn"
                  :aria-expanded="expanded.has(d.key)"
                  @click.prevent="toggleBasis(d.key)"
                >
                  依据
                  <AppIcon
                    :name="expanded.has(d.key) ? 'chevron-down' : 'chevron-right'"
                    :size="12"
                  />
                </button>
                <button class="del" title="不要这一条" @click.prevent="removeDir(c, d)">
                  <AppIcon name="x" :size="11" />
                </button>
              </template>
            </label>
            <p v-if="d.error" class="drow-err">{{ d.error }}</p>
            <p v-if="expanded.has(d.key)" class="basis">{{ d.basis }}</p>
          </li>
          <li v-if="c.dirs.length === 0" class="empty">这套是空的</li>
        </ul>
        <button class="add-dir" @click="addDir(c)">
          <AppIcon name="plus" :size="11" /> 再加一条
        </button>
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

/* ---- 补充要求 ---- */
.ask {
  max-width: 760px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ask-label {
  font-size: 12px;
  font-weight: 500;
  color: var(--c-text-2);
}
.ask-dim {
  font-weight: 400;
  color: var(--c-text-3);
}
.ask-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}
.hint {
  flex: 1;
  min-width: 0;
  resize: vertical;
  border: 0.5px solid var(--c-border-strong);
  border-radius: var(--r-field);
  background: var(--c-surface);
  padding: 8px 10px;
  font-family: var(--font);
  font-size: 12.5px;
  line-height: 1.65;
  color: var(--c-text);
}
.hint::placeholder {
  color: var(--c-text-3);
}
.hint:focus {
  outline: none;
  border-color: var(--c-primary);
  box-shadow: 0 0 0 3px var(--c-primary-bg);
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
  padding: 14px 16px 10px;
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
  padding: 6px;
  font-size: 13px;
  cursor: default;
}

.dname {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border-bottom: 1px dashed transparent;
}
.dname:hover {
  border-bottom-color: var(--c-border-strong);
}
.dname.bad {
  color: var(--c-danger);
}

.dname-input {
  flex: 1;
  min-width: 0;
  height: 26px;
  border-radius: 7px;
  border: 1px solid var(--c-primary);
  background: var(--c-surface);
  padding: 0 7px;
  font-size: 12.5px;
  font-family: var(--font);
}

.dcount {
  font-size: 11px;
  color: var(--c-text-3);
  flex: 0 0 auto;
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
  flex: 0 0 auto;
}
.basis-btn:hover {
  background: var(--c-primary-bg);
}

.del {
  border: 0;
  background: transparent;
  color: var(--c-text-2);
  padding: 2px;
  border-radius: 5px;
  line-height: 0;
  flex: 0 0 auto;
}
.del:hover {
  color: var(--c-danger);
  background: var(--c-danger-bg);
}

.drow-err {
  margin: 0 6px 6px 30px;
  font-size: 11px;
  color: var(--c-danger);
}

.basis {
  margin: 0 6px 8px 30px;
  font-size: 12px;
  color: var(--c-text-2);
  line-height: 1.65;
  padding-left: 8px;
  border-left: 2px solid var(--c-border-strong);
}

.add-dir {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  margin: 4px 0 0 2px;
  border: 0;
  background: transparent;
  color: var(--c-text-2);
  font-size: 11px;
  padding: 4px 6px;
  border-radius: 6px;
}
.add-dir:hover {
  background: var(--c-hover);
  color: var(--c-primary);
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
