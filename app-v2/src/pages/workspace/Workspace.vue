<script setup lang="ts">
/**
 * 整理台（T6.9 骨架 + T6.14 判定与实时反馈）
 *
 * 键盘是主通道：回车确认 / 1-3 选备选 / N 新建 / S 暂缓 / P 先放着 /
 * ↑↓ 换卡 / Ctrl+Z 撤销。所有按钮都同时可点，鼠标用户不会被落下。
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import FileCard from '../../components/FileCard.vue'
import NewDirInput from '../../components/NewDirInput.vue'
import EmptyState from '../../components/EmptyState.vue'
import { mapKey } from '../../core/keys'
import { statusLabel } from '../../core/format'
import { livePath } from '../../core/types'
import { toCmdError } from '../../core/cmdError'
import { useNarrow } from '../../core/useNarrow'
import * as ipc from '../../core/ipc'
import { useAppStore } from '../../stores/app'
import { useBatchStore } from '../../stores/batch'
import { useDirsStore } from '../../stores/dirs'
import type { BatchItem, FilePreview } from '../../core/types'

const router = useRouter()
const app = useAppStore()
const store = useBatchStore()
/** 目录清单：既是 N 输入框的联想源，也是「目录从 0 变成有」的判定依据 */
const dirs = useDirsStore()

const busy = ref(false)
const editingNote = ref<number | null>(null)
const noteDraft = ref('')
/** 打开/定位失败时的提示（不塞进 store.error，那是批次加载的错） */
const actionErr = ref<string | null>(null)
/** 窗口窄时右栏收成抽屉，靠顶部按钮开合 */
const narrow = useNarrow()
const drawer = ref(false)

/**
 * 预览缓存：一份文件一次会话只读一次磁盘。
 *
 * 用 Map 的 key 是 fileId，值允许是 null（读过但没结果）—— 这样「读失败」也不会
 * 每切一次卡就重试一次，把界面拖住。
 */
const previews = ref<Map<number, FilePreview | null>>(new Map())
const currentPreview = computed(() =>
  store.current ? (previews.value.get(store.current.file.id) ?? null) : null,
)

const currentAck = computed(() =>
  store.current && store.lastAck?.fileId === store.current.file.id ? store.lastAck : null,
)

const parkedInBatch = computed(() => store.items.filter((i) => i.file.status === 'parked'))

/** 本批还剩几份没定 —— 头部只报这个数，不报「第几批」（那对用户没有任何意义） */
const pendingCount = computed(
  () => store.items.filter((i) => i.file.status !== 'confirmed').length,
)

/** 刚重算完的一句话反馈（别弹窗，就一行小字） */
const rescoreNote = ref<string | null>(null)

/* ---- BUG-007：删除 = 移入隔离区（二次确认 + 成功提示里的撤销入口） ---- */
interface TrashTarget {
  fileId: number
  name: string
  absPath: string
  relPath: string
}
const trashTarget = ref<TrashTarget | null>(null)
const trashNotice = ref<{ name: string; migrationId: number } | null>(null)

/** 与后端 plan::quarantine_path 同一布局；这里只做展示，真正落点以后端回执为准 */
const trashTargetPath = computed(() => {
  const t = trashTarget.value
  if (!t) return ''
  const root = (app.archiveRoot ?? '').replace(/[\\/]+$/, '')
  if (!root) return '（尚未配置归档根）'
  const rel = t.relPath || t.name
  return `${root}/.filebutler/quarantine/${store.batchId}/${rel}`
})

function askTrash(it: BatchItem) {
  trashTarget.value = {
    fileId: it.file.id,
    name: it.file.name,
    absPath: it.file.absPath,
    relPath: it.file.relPath,
  }
}

function askTrashCurrent() {
  if (store.current) askTrash(store.current)
}

function cancelTrash() {
  trashTarget.value = null
}

async function confirmTrash() {
  const t = trashTarget.value
  if (!t) return
  trashTarget.value = null
  await run(async () => {
    const out = await store.trash(t.fileId)
    if (out) trashNotice.value = { name: t.name, migrationId: out.migrationId }
  })
}

async function undoTrash() {
  const n = trashNotice.value
  if (!n) return
  if (await store.undoTrash(n.migrationId)) trashNotice.value = null
}

// Ctrl+Z 走 store.undo() 时也会清掉 trashUndo；提示条跟着消失，不留给已撤销的删除
watch(
  () => store.trashUndo,
  (rec) => {
    if (!rec) trashNotice.value = null
  },
)

/**
 * 全库还剩多少份没定去向（排队 + 已标签 + 待确认）。
 *
 * 这是「还有多少文件待整理」的唯一口径 —— 批内那个 `remainingInPool` 只是
 * 开批那一刻的池底，用户要的是「我总共还要定多少份」。
 */
const poolLeft = computed(() => app.pendingTotal)

onMounted(async () => {
  // 有批次就接着用：切走再回来不该重来一遍（2026-11-20 反馈）
  await store.ensureLoaded(12)
  void syncDirs()
  // 「还有 N 份待整理」读的是全库计数，进页面得跟库对一次
  void app.refreshBoot()
  window.addEventListener('keydown', onKey)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
})

/** 目录数跟库对齐：批次是复用的，dirCount 还是开批时那个数字 */
async function syncDirs() {
  await dirs.load()
  store.syncDirCount(dirs.count)
}

/** 换卡就取一次预览；同一份文件命中缓存，不再读磁盘 */
watch(
  () => store.current?.file.id,
  (id) => void ensurePreview(id),
  { immediate: true },
)

async function ensurePreview(id: number | undefined) {
  if (!id || previews.value.has(id)) return
  try {
    const p = await ipc.previewFile(id)
    const next = new Map(previews.value)
    next.set(id, p)
    previews.value = next
  } catch {
    // 预览只是锦上添花：读不出来也得能继续判定
    const next = new Map(previews.value)
    next.set(id, null)
    previews.value = next
  }
}

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
      } else if (drawer.value) {
        drawer.value = false
      }
      break
    case 'undo':
      e.preventDefault()
      await store.undo()
      break
    case 'trash':
      // BUG-007：Delete 只负责弹二次确认，取消就不写任何东西
      e.preventDefault()
      askTrashCurrent()
      break
    case 'open':
      e.preventDefault()
      await openCurrent()
      break
    case 'reveal':
      e.preventDefault()
      await revealCurrent()
      break
  }
}

/** 用系统默认程序打开当前文件（Word/Excel/看图器都行） */
async function openCurrent() {
  const it = store.current
  if (!it) return
  actionErr.value = null
  try {
    await ipc.openPath(livePath(it.file))
  } catch (e) {
    actionErr.value = toCmdError(e).message
  }
}

/** 在资源管理器中定位（文件已经不在了会由后端报错） */
async function revealCurrent() {
  const it = store.current
  if (!it) return
  actionErr.value = null
  try {
    await ipc.revealPath(livePath(it.file))
  } catch (e) {
    actionErr.value = toCmdError(e).message
  }
}

async function run(fn: () => Promise<unknown>) {
  busy.value = true
  try {
    await fn()
    await app.refreshBoot()
    // 判定可能刚建过目录（N 那条路），目录数要跟上
    await syncDirs()
  } finally {
    busy.value = false
  }
}

/** 手动按现在的目录集合重算候选 */
async function rescore() {
  busy.value = true
  rescoreNote.value = null
  try {
    const changed = await store.refreshCandidates()
    rescoreNote.value = changed
      ? `已按现有目录重算，${changed} 份的首选换了`
      : '已按现有目录重算，首选没变'
  } finally {
    busy.value = false
  }
}

/** 卡片上改归档名：只改目标名，源文件不动 */
async function onRename(name: string) {
  const it = store.current
  if (!it) return
  await store.renameTarget(it.file.id, name)
}

/** 卡片上加标签：后端落 kind='user' 并锁定，前端就地补一颗（不整批重拉） */
async function onAddTag(name: string) {
  const it = store.current
  if (!it) return
  try {
    const t = await ipc.addTag(it.file.id, name)
    it.file.tags = [...(it.file.tags ?? []), t]
    it.file.tagLocked = true
  } catch (e) {
    actionErr.value = String(e)
  }
}

/** 删标签：就地摘掉；type/time 后端会拒，拒了就把错误亮出来 */
async function onRemoveTag(tagId: number) {
  const it = store.current
  if (!it) return
  try {
    await ipc.removeTag(it.file.id, tagId)
    it.file.tags = (it.file.tags ?? []).filter((t) => t.id !== tagId)
    it.file.tagLocked = true
  } catch (e) {
    actionErr.value = String(e)
  }
}

/** Ctrl+点卡片标签 → 高级筛选预置该标签（T3.6；普通点击不跳，防整理台误触） */
function onTagClick(tagId: number) {
  router.push({ path: '/search', query: { tag: String(tagId) } })
}

async function onNewDirConfirm(input: string) {
  store.closeInput()
  await run(() => store.newDirConfirm(input))
}

async function reload() {
  busy.value = true
  // 换一批等于「磁盘可能变了」，预览缓存一起作废
  previews.value = new Map()
  actionErr.value = null
  try {
    await store.load(12)
    // 当前这份的 id 可能没变，watch 不会触发，所以显式补一次
    void ensurePreview(store.current?.file.id)
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
  <div class="ws" :class="{ narrow }">
    <header class="top">
      <div class="title">
        <template v-if="store.total > 0">
          待确认 <b>{{ pendingCount }}</b> 份
        </template>
        <template v-else>整理台</template>
      </div>
      <div class="acts">
        <span v-if="rescoreNote" class="note">{{ rescoreNote }}</span>
        <!-- 「还有多少文件待整理」以前只在批次里说「池里还有 N 份」，
             池子到底多大、还剩多少得靠猜。这里给全库口径，跟着判定实时减 -->
        <span v-if="poolLeft > 0" class="rest">还有 {{ poolLeft }} 份待整理</span>
        <span v-else-if="store.total > 0" class="rest done">全部整理完了</span>
        <!-- 窄窗时右栏收进抽屉，这里给一个入口（宽窗不需要，右栏一直在） -->
        <button
          v-if="narrow"
          class="btn drawer-btn"
          :class="{ on: drawer }"
          :aria-expanded="drawer"
          @click="drawer = !drawer"
        >
          <AppIcon name="list" :size="12" />
          已确认 {{ store.confirmedCount }}
          <template v-if="parkedInBatch.length">/ 放着 {{ parkedInBatch.length }}</template>
        </button>
        <button
          class="btn"
          :disabled="busy || store.undoStack.length === 0"
          :title="store.undoStack.length ? '撤销最近一次确认（Ctrl+Z）' : '还没有可撤销的确认'"
          @click="undoOnce"
        >
          <AppIcon name="undo" :size="12" /> 撤销{{ store.undoStack.length ? `(${store.undoStack.length})` : '' }}
        </button>
        <button
          class="btn"
          :disabled="busy || store.total === 0"
          title="按现在的目录重新算一遍候选（新建目录之后用这个）"
          @click="rescore"
        >
          重新推荐
        </button>
        <button class="btn" :disabled="busy" @click="reload">换一批</button>
        <button v-if="store.total > 0" class="btn" @click="finishBatch">结束本批</button>
      </div>
    </header>

    <p v-if="store.error || actionErr" class="err" role="alert">{{ store.error ?? actionErr }}</p>

    <!-- 0 目录：一行提示就够。之前这里是一整块 1009×241 的虚线空态，
         直接压在已经完整可用的整理台上，除了占地方没有任何作用 -->
    <div v-if="store.dirCount === 0 && store.total > 0" class="nodir">
      <AppIcon name="sparkle" :size="14" />
      <span class="nd-t">还没有任何目录</span>
      <span class="nd-d">先建一个才能归档；也可以让 AI 按这批文件给两套方案</span>
      <button class="btn primary small" @click="goSuggest">让 AI 建议目录结构</button>
      <button class="btn small" @click="store.openInput()">自己写一个</button>
    </div>

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
            <!-- 队列里的都是待确认的，这一列只给「已确认 / 暂缓」留位置，
                 别每行重复写一遍「待确认」占掉文件名的地方 -->
            <span v-if="it.file.status !== 'pending_confirm'" class="qst">
              {{ statusLabel(it.file.status) }}
            </span>
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
          :preview="currentPreview"
          @accept="run(() => store.accept())"
          @pick="run(() => store.pick($event))"
          @new-dir="store.openInput()"
          @defer="run(() => store.defer())"
          @park="run(() => store.park())"
          @trash="askTrashCurrent"
          @open="openCurrent"
          @reveal="revealCurrent"
          @rename="onRename"
          @add-tag="onAddTag"
          @remove-tag="onRemoveTag"
          @tag-click="onTagClick"
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
          <span><kbd>Del</kbd> 删除（移入隔离区）</span>
          <span><kbd>↑</kbd><kbd>↓</kbd> 换一份</span>
          <span><kbd>O</kbd> 打开</span>
          <span><kbd>L</kbd> 定位</span>
          <span><kbd>Ctrl</kbd>+<kbd>Z</kbd> 撤销</span>
        </p>
      </main>

      <!-- 右：本批已确认（窄窗时是抽屉，见 .ws.narrow） -->
      <aside v-if="!narrow || drawer" class="right">
        <h3>本批已确认 {{ store.confirmedList.length }}</h3>
        <TransitionGroup tag="ul" name="rise" class="done">
          <li v-for="it in store.confirmedList" :key="it.file.id" class="drow">
            <AppIcon name="check" :size="12" />
            <span class="dname ellipsis" :title="it.file.name">{{ it.file.name }}</span>
            <button
              class="dtrash"
              title="移入隔离区，可撤销（Delete）；原文件可取回"
              @click="askTrash(it)"
            >删除</button>
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

    <!-- BUG-007：删除（移入隔离区）二次确认。取消零写操作；文案写明落点与可撤销性 -->
    <div v-if="trashTarget" class="dlg-mask" @click.self="cancelTrash">
      <div class="dlg" role="dialog" aria-modal="true" aria-label="确认移入隔离区">
        <h3>删除（移入隔离区）</h3>
        <p>
          确认把「<b>{{ trashTarget.name }}</b>」移入隔离区吗？
        </p>
        <p class="dlg-dest">目标路径：{{ trashTargetPath }}</p>
        <p class="dlg-tip">文件将移入隔离区，可撤销；取消则不执行任何操作。</p>
        <div class="dlg-acts">
          <button class="btn" @click="cancelTrash">取消</button>
          <button class="btn primary" @click="confirmTrash">确认移入隔离区</button>
        </div>
      </div>
    </div>

    <!-- BUG-007：删除成功提示，撤销入口直通 undo_trash -->
    <div v-if="trashNotice" class="trash-notice" role="status">
      <AppIcon name="check" :size="14" />
      <span>已将「{{ trashNotice.name }}」移入隔离区，可撤销</span>
      <button class="btn small" @click="undoTrash">撤销</button>
      <button class="btn small" @click="trashNotice = null">关闭</button>
    </div>

    <!-- 完成提示条：非模态，不挡手（详设 §8.3） -->
    <div v-if="store.allDone && store.total > 0" class="donebar">
      <AppIcon name="check" :size="14" />
      <span>本批 {{ store.total }} 份处理完了</span>
      <span class="dim">池里还有 {{ store.finished?.remainingInPool ?? store.remaining }} 份</span>
      <button class="btn small" @click="reload">再来一批</button>
      <button class="btn small" @click="router.push('/migration')">去待迁移清单</button>
      <!-- 有 parked 才显示，且带数字：光写「去待定区看看」等于没说有多少活儿（T4.4） -->
      <button v-if="app.parkedCount > 0" class="btn small" @click="router.push('/parked')">
        去待定区看看（{{ app.parkedCount }}）
      </button>
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
  font-variant-numeric: tabular-nums;
}
.rest.done {
  color: var(--c-success-deep);
}

.note {
  font-size: 12px;
  color: var(--c-success-deep);
}

/* 0 目录提示条：一行，不再占掉半屏 */
.nodir {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px 7px 12px;
  border-radius: var(--r-banner);
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-size: 12px;
}
.nd-t {
  font-weight: 500;
  flex: 0 0 auto;
}
.nd-d {
  min-width: 0;
  flex: 1 1 auto;
  color: var(--c-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nodir .btn {
  flex: 0 0 auto;
}

.cols {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 208px minmax(0, 1fr) 208px;
  gap: 14px;
  position: relative;
}

/* 窄窗：右栏收成抽屉，两栏并排。中间卡片从 ~468px 回到 ~640px（1160 默认窗口下） */
.ws.narrow .cols {
  grid-template-columns: 176px minmax(0, 1fr);
}

.ws.narrow .right {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: 264px;
  z-index: 6;
  box-shadow: -10px 0 28px rgba(0, 0, 0, 0.1);
  animation: drawer-in var(--dur) var(--ease-out) both;
}
@keyframes drawer-in {
  from {
    opacity: 0;
    transform: translateX(12px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

.drawer-btn.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
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

/* BUG-005：固定行高下限 + 不参与 flex 伸缩 → 2 条与 10 条行高完全一致 */
.drow {
  display: grid;
  grid-template-columns: 12px minmax(0, 1fr);
  align-items: center;
  column-gap: 6px;
  padding: 4px 6px;
  min-height: 24px;
  flex: 0 0 auto;
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

/* 卡片按内容高度，不再拉满：已确认状态内容很少，拉满会变成「上面挤一堆字、
   下面一大片空」。快捷键行靠 margin-top:auto 自己贴底 */
.mid :deep(.card) {
  flex: 0 0 auto;
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

/* ---- BUG-007：删除二次确认 / 成功提示 / 行内删除入口 ---- */
.dlg-mask {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  place-items: center;
  background: rgba(0, 0, 0, 0.36);
}
.dlg {
  width: min(520px, calc(100vw - 40px));
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.22);
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.dlg h3 {
  font-size: 15px;
  color: var(--c-text);
}
.dlg p {
  margin: 0;
  font-size: 13px;
  color: var(--c-text);
  line-height: 1.6;
}
.dlg-dest {
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 11px !important;
  color: var(--c-text-2) !important;
  word-break: break-all;
  padding: 8px 10px;
  border-radius: var(--r-item);
  background: var(--c-field);
}
.dlg-tip {
  color: var(--c-text-2) !important;
  font-size: 12px !important;
}
.dlg-acts {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}
.trash-notice {
  position: sticky;
  bottom: 0;
  z-index: 8;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 14px;
  border-radius: var(--r-banner);
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.08);
  font-size: 12px;
}
.trash-notice .btn:first-of-type {
  margin-left: auto;
}
.dtrash {
  flex: 0 0 auto;
  border: 0;
  background: transparent;
  padding: 0 3px;
  font-size: 10px;
  font-family: var(--font);
  color: var(--c-text-3);
}
.dtrash:hover {
  color: var(--c-danger);
}
</style>
