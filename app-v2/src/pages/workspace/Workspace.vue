<script setup lang="ts">
/**
 * 整理台（T6.9 骨架 + T6.14 判定与实时反馈）
 *
 * 键盘是主通道：回车确认 / 1-3 选备选 / N 新建 / S 暂缓 / P 先放着 /
 * ↑↓ 换卡 / Ctrl+Z 撤销。所有按钮都同时可点，鼠标用户不会被落下。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import FileCard from '../../components/FileCard.vue'
import NewDirInput from '../../components/NewDirInput.vue'
import EmptyState from '../../components/EmptyState.vue'
import { mapKey } from '../../core/keys'
import { statusLabel } from '../../core/format'
import { useAppStore } from '../../stores/app'
import { useBatchStore } from '../../stores/batch'

const router = useRouter()
const app = useAppStore()
const store = useBatchStore()

const busy = ref(false)
const editingNote = ref<number | null>(null)
const noteDraft = ref('')

const currentAck = computed(() =>
  store.current && store.lastAck?.fileId === store.current.file.id ? store.lastAck : null,
)

const parkedInBatch = computed(() => store.items.filter((i) => i.file.status === 'parked'))

onMounted(async () => {
  await store.load(12)
  void app.refreshBoot()
  window.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
})

async function onKey(e: KeyboardEvent) {
  // 文本输入优先：焦点在 input/textarea 时只放 Esc/Enter 出去
  const el = document.activeElement as HTMLElement | null
  const inField =
    el?.tagName === 'INPUT' || el?.tagName === 'TEXTAREA' || el?.isContentEditable === true
  const cmd = mapKey(e, { inputOpen: inField || store.inputOpen })
  if (!cmd) return
  // 已经在输入框里：Esc/Enter 由组件自己处理，这里不抢
  if (inField && cmd.t !== 'escape' && cmd.t !== 'accept') return

  switch (cmd.t) {
    case 'accept':
      if (store.inputOpen) return
      e.preventDefault()
      await run(() => store.accept())
      break
    case 'pick':
      e.preventDefault()
      await run(() => store.pick(cmd.n))
      break
    case 'newDir':
      e.preventDefault()
      store.openInput()
      break
    case 'defer':
      e.preventDefault()
      await run(() => store.defer())
      break
    case 'park':
      e.preventDefault()
      await run(() => store.park())
      break
    case 'move':
      e.preventDefault()
      store.move(cmd.d)
      break
    case 'escape':
      if (store.inputOpen) {
        e.preventDefault()
        store.closeInput()
      }
      break
    case 'undo':
      e.preventDefault()
      await store.undo()
      break
  }
}

async function run(fn: () => Promise<unknown>) {
  busy.value = true
  try {
    await fn()
    void app.refreshBoot()
  } finally {
    busy.value = false
  }
}

async function onNewDirConfirm(input: string) {
  store.closeInput()
  await run(() => store.newDirConfirm(input))
}

async function reload() {
  busy.value = true
  try {
    await store.load(12)
  } finally {
    busy.value = false
  }
}

async function undoOnce() {
  await store.undo()
  void app.refreshBoot()
}

async function finishBatch() {
  await store.finish()
  void app.refreshBoot()
}

function startEditNote(fileId: number, note: string | null) {
  editingNote.value = fileId
  noteDraft.value = note ?? ''
}

async function commitNote(fileId: number) {
  const v = noteDraft.value.trim()
  editingNote.value = null
  if (v) await store.saveNote(fileId, v)
}

function goSuggest() {
  void router.push('/onboarding/dirs')
}
</script>

<template>
  <div class="ws">
    <header class="top">
      <div class="title">
        <template v-if="store.total > 0">
          第 {{ store.seq }} 批
          <span class="sep">·</span>
          {{ store.total }} 份
          <span class="sep">·</span>
          已确认 <b>{{ store.confirmedCount }}</b
          >/{{ store.total }}
        </template>
        <template v-else>整理台</template>
      </div>
      <div class="acts">
        <span v-if="store.remaining > 0" class="rest">池里还有 {{ store.remaining }} 份</span>
        <button
          class="btn"
          :disabled="busy || store.undoStack.length === 0"
          :title="store.undoStack.length ? '撤销最近一次确认（Ctrl+Z）' : '还没有可撤销的确认'"
          @click="undoOnce"
        >
          <AppIcon name="undo" :size="12" /> 撤销{{ store.undoStack.length ? `(${store.undoStack.length})` : '' }}
        </button>
        <button class="btn" :disabled="busy" @click="reload">换一批</button>
        <button v-if="store.total > 0" class="btn" @click="finishBatch">结束本批</button>
      </div>
    </header>

    <p v-if="store.error" class="err" role="alert">{{ store.error }}</p>

    <!-- 0 目录空态（验收 11）：不报错、不阻塞，只换一条路 -->
    <EmptyState
      v-if="store.dirCount === 0 && store.total > 0"
      icon="sparkle"
      title="还没有任何目录"
      desc="先有目录才能归档。可以让 AI 给两套方案，也可以自己写一个。"
    >
      <button class="btn primary" @click="goSuggest">让 AI 建议目录结构</button>
      <button class="btn" @click="store.openInput()">手动新建目录</button>
    </EmptyState>

    <div v-if="store.total > 0" class="cols">
      <!-- 左：队列 -->
      <aside class="left">
        <h3>本批队列</h3>
        <ul class="queue">
          <li
            v-for="(it, i) in store.items"
            :key="it.file.id"
            class="qrow"
            :class="{ on: i === store.index, done: it.file.status === 'confirmed' }"
            @click="store.focus(i)"
          >
            <span class="no">{{ i + 1 }}</span>
            <span class="qname ellipsis" :title="it.file.name">{{ it.file.name }}</span>
            <span class="qst">{{ statusLabel(it.file.status) }}</span>
          </li>
        </ul>
      </aside>

      <!-- 中：当前卡 -->
      <main class="mid">
        <!-- key 换 → 组件重挂 → 卡片自带走入场动画（见 FileCard .card 的 animation）。
             不用 <Transition> 是刻意的：out-in 期间容器会塌一下，下面那行快捷键会跟着跳 -->
        <FileCard
          v-if="store.current"
          :key="store.current.file.id"
          :item="store.current"
          :dir-count="store.dirCount"
          :position="{ index: store.index, total: store.total }"
          :deferred-badge="store.deferredBadge.has(store.current.file.id)"
          :ack="currentAck"
          :busy="busy"
          @accept="run(() => store.accept())"
          @pick="run(() => store.pick($event))"
          @new-dir="store.openInput()"
          @defer="run(() => store.defer())"
          @park="run(() => store.park())"
        />

        <div v-if="store.inputOpen" class="newdir">
          <NewDirInput
            placeholder="新目录名，多级用 / 分隔"
            @confirm="onNewDirConfirm"
            @cancel="store.closeInput()"
          />
        </div>

        <p class="keys">
          <span><kbd>回车</kbd> 确认主推</span>
          <span><kbd>1</kbd><kbd>2</kbd><kbd>3</kbd> 选备选</span>
          <span><kbd>N</kbd> 新建目录</span>
          <span><kbd>S</kbd> 暂缓</span>
          <span><kbd>P</kbd> 先放着</span>
          <span><kbd>↑</kbd><kbd>↓</kbd> 换一份</span>
          <span><kbd>Ctrl</kbd>+<kbd>Z</kbd> 撤销</span>
        </p>
      </main>

      <!-- 右：本批已确认 -->
      <aside class="right">
        <h3>本批已确认 {{ store.confirmedList.length }}</h3>
        <TransitionGroup tag="ul" name="rise" class="done">
          <li v-for="it in store.confirmedList" :key="it.file.id" class="drow">
            <AppIcon name="check" :size="12" />
            <span class="dname ellipsis" :title="it.file.name">{{ it.file.name }}</span>
          </li>
          <li v-if="store.confirmedList.length === 0" class="none">还没确认任何一份</li>
        </TransitionGroup>

        <template v-if="parkedInBatch.length > 0">
          <h3 class="mt">先放着 {{ parkedInBatch.length }}</h3>
          <ul class="done">
            <li
              v-for="it in parkedInBatch"
              :key="it.file.id"
              class="prow"
              @dblclick="startEditNote(it.file.id, it.file.note)"
            >
              <span class="dname ellipsis" :title="it.file.name">{{ it.file.name }}</span>
              <input
                v-if="editingNote === it.file.id"
                v-model="noteDraft"
                class="note-input"
                placeholder="备注（回车保存）"
                @keydown.enter.stop.prevent="commitNote(it.file.id)"
                @keydown.esc.stop.prevent="editingNote = null"
                @blur="commitNote(it.file.id)"
              />
              <span v-else class="pnote ellipsis">{{ it.file.note ?? '双击写备注' }}</span>
            </li>
          </ul>
        </template>
      </aside>
    </div>

    <EmptyState
      v-else-if="!store.loading"
      icon="check"
      title="池子里没有待整理的文件了"
      :desc="
        app.counts.confirmed > 0
          ? '确认过的文件在「待迁移」里等着，检查一遍就能搬。'
          : '先去扫一遍文件夹，或换一个源区。'
      "
    >
      <button v-if="app.counts.confirmed > 0" class="btn primary" @click="router.push('/migration')">
        去待迁移清单
      </button>
      <button v-else class="btn" @click="router.push('/settings')">去设置里扫描</button>
    </EmptyState>

    <!-- 完成提示条：非模态，不挡手（详设 §8.3） -->
    <div v-if="store.allDone && store.total > 0" class="donebar">
      <AppIcon name="check" :size="14" />
      <span>本批 {{ store.total }} 份处理完了</span>
      <span class="dim">池里还有 {{ store.finished?.remainingInPool ?? store.remaining }} 份</span>
      <button class="btn small" @click="reload">再来一批</button>
      <button class="btn small" @click="router.push('/migration')">去待迁移清单</button>
      <button class="btn small" @click="router.push('/parked')">去待定区看看</button>
    </div>
  </div>
</template>

<style scoped>
.ws {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.top {
  display: flex;
  align-items: center;
  gap: 12px;
  flex: 0 0 auto;
}

.title {
  font-size: 15px;
  font-weight: 600;
}
.title .sep {
  color: var(--c-text-3);
  font-weight: 400;
}

.acts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
}

.rest {
  font-size: 12px;
  color: var(--c-text-2);
}

.cols {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 208px minmax(0, 1fr) 208px;
  gap: 14px;
}

.left,
.right {
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  padding: 12px 12px 10px;
  overflow: auto;
}

h3 {
  margin: 0;
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.3px;
  color: var(--c-text-2);
}
h3.mt {
  margin-top: 10px;
}

.queue,
.done {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.qrow {
  display: grid;
  grid-template-columns: 16px minmax(0, 1fr) auto;
  align-items: center;
  column-gap: 7px;
  padding: 5px 7px;
  border-radius: var(--r-item);
  font-size: 12px;
  cursor: default;
  overflow: hidden;
  transition: background var(--dur-fast) ease, color var(--dur-fast) ease;
}
.qrow:hover {
  background: var(--c-hover);
}
.qrow.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-weight: 500;
}
.qrow.done {
  color: var(--c-text-2);
  background: var(--c-success-bg);
}

.no {
  flex: 0 0 14px;
  font-size: 10px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}

.qname,
.dname {
  min-width: 0;
  max-width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.qst {
  font-size: 10px;
  color: var(--c-text-3);
  flex: 0 0 auto;
  white-space: nowrap;
}

.drow {
  display: grid;
  grid-template-columns: 12px minmax(0, 1fr);
  align-items: center;
  column-gap: 6px;
  padding: 4px 6px;
  font-size: 12px;
  color: var(--c-success-deep);
}

.prow {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 5px 6px;
  border-radius: var(--r-item);
  font-size: 12px;
}
.prow:hover {
  background: var(--c-hover);
}
.pnote {
  font-size: 11px;
  color: var(--c-text-3);
}
.note-input {
  height: 26px;
  border-radius: 7px;
  border: 1px solid var(--c-primary);
  background: var(--c-surface);
  padding: 0 7px;
  font-size: 12px;
  font-family: var(--font);
}

.none {
  padding: 6px;
  font-size: 12px;
  color: var(--c-text-3);
}

.mid {
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.newdir {
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  padding: 12px 14px;
}

.keys {
  margin: auto 0 0;
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 11px;
  color: var(--c-text-3);
  padding-top: 2px;
}

kbd {
  font-family: var(--font);
  font-size: 10px;
  padding: 1px 5px;
  margin-right: 2px;
  border-radius: 5px;
  border: 0.5px solid var(--c-border-strong);
  background: var(--c-surface);
  color: var(--c-text-2);
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: 0;
  border-radius: var(--r-btn);
  padding: 6px 12px;
  font-size: 12px;
  background: var(--c-field);
  color: var(--c-text);
  font-family: var(--font);
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn.small {
  padding: 4px 10px;
}
.btn:disabled {
  opacity: 0.55;
}

.err {
  margin: 0;
  padding: 8px 12px;
  border-radius: var(--r-banner);
  background: var(--c-danger-bg);
  color: var(--c-danger);
  font-size: 12px;
}

.donebar {
  position: sticky;
  bottom: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-radius: var(--r-banner);
  background: var(--c-surface);
  border: 0.5px solid rgba(40, 200, 64, 0.35);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.08);
  font-size: 12px;
  color: var(--c-success-deep);
}
.donebar .dim {
  color: var(--c-text-2);
}
.donebar .btn {
  margin-left: 0;
}
.donebar .btn:nth-of-type(1) {
  margin-left: auto;
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
