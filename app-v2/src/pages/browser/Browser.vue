<script setup lang="ts">
/**
 * 浏览页（P3 T2.4 / T2.5）—— 归档区的唯一入口。
 *
 * 左树：两个虚拟视图（待整理 / 待定区，`virtual:pool` / `virtual:parked`，
 * 不占 archive_dirs 的 id 空间）+ 归档目录树（带直属份数）。
 * 右列表：名称 / 类型 / 大小 / 时间 / 标签 / 状态 六格（标签进自己的格，
 * 不另加第七格 —— alpha.5 的「4 格变 5 格」教训）。
 * 交互：单击选行出详情条；双击打开；右键菜单**只有三项**
 * （打开 / 定位 / 复制路径）——「重新归类」留 P4，界面上不出现假按钮。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import DirTree from '../../components/DirTree.vue'
import FileList from '../../components/FileList.vue'
import { useNarrow } from '../../core/useNarrow'
import { iconOf } from '../../core/format'
import { livePath } from '../../core/types'
import type { DirNode, FileRow } from '../../core/types'
import * as ipc from '../../core/ipc'

const router = useRouter()
/** 窄窗（≤1240px）：左树收成抽屉（T5.3，与整理台右栏同一手法） */
const narrow = useNarrow()
const drawer = ref(false)

/** 左树选中的节点：虚拟视图（pool / parked / uncategorized）或目录 id */
type Sel = { kind: 'pool' | 'parked' | 'uncat' | 'dir'; id: number | null }

const loading = ref(true)
const tree = ref<DirNode[]>([])
const rows = ref<FileRow[]>([])
const pool = ref<FileRow[]>([])
const parkedRows = ref<FileRow[]>([])
/** 折叠的目录 id */
const collapsed = ref<Set<number>>(new Set())
/** 是否含子目录 */
const includeSub = ref(false)
const selected = ref<Sel>({ kind: 'dir', id: null })
/** 当前选中行（详情条） */
const activeId = ref<number | null>(null)
/** 右键菜单 */
const menu = ref<{ x: number; y: number; row: FileRow } | null>(null)

const active = computed(() => rows.value.find((r) => r.id === activeId.value) ?? null)

function toggle(id: number) {
  const next = new Set(collapsed.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  collapsed.value = next
}

async function loadList() {
  loading.value = true
  try {
    if (selected.value.kind === 'pool') {
      rows.value = pool.value
    } else if (selected.value.kind === 'parked') {
      rows.value = parkedRows.value
    } else if (selected.value.kind === 'uncat') {
      rows.value = await ipc.browseDirFiles(null, includeSub.value)
    } else {
      rows.value = await ipc.browseDirFiles(selected.value.id, includeSub.value)
    }
  } finally {
    loading.value = false
  }
}

async function select(sel: Sel) {
  selected.value = sel
  activeId.value = null
  filterQ.value = ''
  filtering.value = false
  if (narrow.value) drawer.value = false // 抽屉里点完就走
  clearTimeout(filterTimer)
  await loadList()
}

async function refreshAll() {
  const [t, p, pk] = await Promise.all([ipc.browseTree(), ipc.poolRows(), ipc.listParked()])
  tree.value = t
  pool.value = p
  // listParked 给的是带候选的 ParkedItem，虚拟视图只要文件行
  parkedRows.value = pk.map((x) => x.file)
  await loadList()
}

onMounted(refreshAll)

/* ---- 打开 / 定位 / 复制 ---- */
async function openRow(r: FileRow) {
  try {
    await ipc.openPath(livePath(r))
  } catch {
    menu.value = null
  }
}
async function revealRow(r: FileRow) {
  try {
    await ipc.revealPath(livePath(r))
  } catch {
    menu.value = null
  }
}
async function copyPath(r: FileRow) {
  try {
    await navigator.clipboard.writeText(livePath(r))
  } catch {
    /* 剪贴板被占就不复制，不弹错 */
  }
  menu.value = null
}

function rowMenu(e: MouseEvent, r: FileRow) {
  e.preventDefault()
  menu.value = { x: e.clientX, y: e.clientY, row: r }
}
function closeMenu() {
  menu.value = null
}

function goWorkspace() {
  router.push('/')
}

/** 标签 chip → 高级筛选预置该标签（T3.6） */
function goTag(tagId: number) {
  router.push({ path: '/search', query: { tag: String(tagId) } })
}

/* ---- 目录内筛选（T3.5）：不跳页，走共用查询层 ---- */
const filterQ = ref('')
const totalShown = computed(() =>
  filterQ.value.trim() ? filteredTotal.value : rows.value.length,
)
const filteredTotal = ref(0)
let filterTimer: ReturnType<typeof setTimeout> | undefined
const filtering = ref(false)

/** 目录只在 archive 语义下可筛；虚拟视图直接走 all 范围的关键词 */
function filterReq() {
  return {
    q: filterQ.value.trim(),
    scope: selected.value.kind === 'dir' || selected.value.kind === 'uncat'
      ? ('archive' as const)
      : ('all' as const),
    dirId: selected.value.kind === 'dir' ? selected.value.id : null,
    includeSubdirs: includeSub.value,
    limit: 200,
  }
}

async function runFilter() {
  const q = filterQ.value.trim()
  if (!q) {
    filtering.value = false
    await loadList()
    return
  }
  filtering.value = true
  const res = await ipc.searchFiles(filterReq())
  rows.value = res.items
  filteredTotal.value = res.total
  loading.value = false
}

function filterDebounced() {
  clearTimeout(filterTimer)
  filterTimer = setTimeout(runFilter, 150)
}

function clearFilter() {
  filterQ.value = ''
  clearTimeout(filterTimer)
  void runFilter()
}

async function onSubToggle() {
  if (filtering.value && filterQ.value.trim()) {
    clearTimeout(filterTimer)
    await runFilter()
  } else {
    await loadList()
  }
}
</script>

<template>
  <div class="browser" :class="{ narrow }" @click="closeMenu">
    <!-- 左树：窄窗时收成抽屉，用「目录」按钮呼出 -->
    <aside v-if="!narrow || drawer" class="side">
      <div class="vhead">入口</div>
      <div
        class="vrow"
        :class="{ on: selected.kind === 'pool' }"
        role="treeitem"
        @click="select({ kind: 'pool', id: null })"
      >
        <AppIcon name="list" :size="14" />
        <span class="vname">待整理</span>
        <span class="cnt">{{ pool.length }}</span>
      </div>
      <div
        class="vrow"
        :class="{ on: selected.kind === 'parked' }"
        role="treeitem"
        @click="select({ kind: 'parked', id: null })"
      >
        <AppIcon name="pause" :size="14" />
        <span class="vname">待定区</span>
        <span class="cnt">{{ parkedRows.length }}</span>
      </div>

      <div class="vhead">归档目录</div>
      <DirTree
        :nodes="tree"
        :selected-id="selected.kind === 'dir' ? selected.id : null"
        :collapsed="collapsed"
        @select="select({ kind: 'dir', id: $event })"
        @toggle="toggle"
      />
      <div
        class="vrow uncat"
        :class="{ on: selected.kind === 'uncat' }"
        role="treeitem"
        @click="select({ kind: 'uncat', id: null })"
      >
        <AppIcon name="folder" :size="14" />
        <span class="vname">未归类</span>
      </div>
    </aside>

    <!-- 右列表 -->
    <section class="main">
      <header class="bar">
        <button v-if="narrow" class="btn ghost drawer-toggle" @click.stop="drawer = !drawer">
          {{ drawer ? '收起' : '目录' }}
        </button>
        <span class="where">
          {{
            selected.kind === 'pool'
              ? '待整理'
              : selected.kind === 'parked'
                ? '待定区'
                : selected.kind === 'uncat'
                  ? '未归类'
                  : (tree.find((n) => n.id === selected.id)?.relPath ?? '')
          }}
        </span>
        <!-- 目录内筛选（T3.5）：请求带 dirId + includeSubdirs，不跳页；清空恢复全量 -->
        <input
          v-model="filterQ"
          class="filter"
          type="text"
          placeholder="在这个目录里找…"
          spellcheck="false"
          @input="filterDebounced"
          @keydown.esc="clearFilter"
        />
        <button v-if="filterQ" class="clearbtn" title="清空筛选" @click="clearFilter">清空</button>
        <label class="sub"><input v-model="includeSub" type="checkbox" @change="onSubToggle" />含子目录</label>
        <span class="total">{{ totalShown }} 份</span>
      </header>

      <FileList
        :rows="rows"
        :active-id="activeId"
        :loading="loading"
        @activate="activeId = $event"
        @open="openRow"
        @menu="rowMenu($event.event, $event.row)"
        @tag-click="goTag($event.tagId)"
      >
        <template #empty>
          <AppIcon name="folder" :size="26" />
          <span class="et">这个目录还没有文件</span>
          <button class="link" @click="goWorkspace">去整理台看看</button>
        </template>
      </FileList>

      <!-- 详情条：单击选行后出现 -->
      <aside v-if="active" class="detail">
        <div class="dhead">
          <AppIcon :name="iconOf(active.kind)" :size="15" />
          <span class="dname ellipsis" :title="active.name">{{ active.name }}</span>
          <button class="x" title="收起详情" @click="activeId = null">×</button>
        </div>
        <p class="dsummary" :class="{ none: !active.summary }">
          {{ active.summary || '未解读' }}
        </p>
        <p v-if="(active.tags ?? []).length > 0" class="dtags">
          <span v-for="t in active.tags" :key="t.id" class="tchip" :class="t.kind">{{ t.name }}</span>
        </p>
        <p v-if="active.note" class="dnote">备注：{{ active.note }}</p>
        <p class="dpath mono" :title="livePath(active)">{{ livePath(active) }}</p>
        <div class="dacts">
          <button class="btn" @click="openRow(active)">打开</button>
          <button class="btn" @click="revealRow(active)">定位</button>
        </div>
      </aside>
    </section>

    <!-- 右键菜单：只有三项，锁死「不出现重新归类」（验收 7） -->
    <div
      v-if="menu"
      class="menu"
      :style="{ left: `${menu.x}px`, top: `${menu.y}px` }"
      @click.stop
    >
      <button class="mi" @click="openRow(menu.row)">打开</button>
      <button class="mi" @click="revealRow(menu.row)">在资源管理器中定位</button>
      <button class="mi" @click="copyPath(menu.row)">复制路径</button>
    </div>
  </div>
</template>

<style scoped>
.browser {
  display: flex;
  gap: 14px;
  height: 100%;
  min-height: 0;
  position: relative;
}

/* 窄窗（≤1240px）：左树收成抽屉，浮在列表上；与整理台右栏同一套手法（T5.3） */
.browser.narrow .side {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  width: 218px;
  z-index: 6;
  background: var(--c-bg);
  box-shadow: 10px 0 28px rgba(0, 0, 0, 0.1);
  animation: drawer-in var(--dur) var(--ease-out) both;
}
@keyframes drawer-in {
  from {
    opacity: 0;
    transform: translateX(-8px);
  }
}
.drawer-toggle {
  flex: 0 0 auto;
  font-size: 12px;
  padding: 0 10px;
}

.side {
  flex: 0 0 218px;
  width: 218px;
  min-height: 0;
  overflow: auto;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  padding: 10px 8px;
}
.vhead {
  font-size: 10px;
  color: var(--c-text-3);
  padding: 8px 8px 4px;
}
.vrow {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 8px;
  border-radius: var(--r-item);
  font-size: 13px;
  color: var(--c-text);
  user-select: none;
  cursor: default;
}
.vrow:hover {
  background: var(--c-hover);
}
.vrow.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-weight: 500;
}
.vrow.uncat {
  margin-top: 8px;
  color: var(--c-text-2);
}
.vname {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cnt {
  font-size: 11px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}

.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
}
.where {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sub {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--c-text-2);
  user-select: none;
}
.total {
  font-size: 12px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}

/* 目录内筛选（T3.5） */
.filter {
  flex: 0 1 220px;
  min-width: 120px;
  height: 28px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid var(--c-border);
  background: var(--c-field);
  font-size: 12px;
  font-family: var(--font);
  color: var(--c-text);
  outline: none;
}
.filter:focus {
  border-color: var(--c-primary);
  box-shadow: 0 0 0 2px var(--c-primary-glow);
}
.clearbtn {
  flex: 0 0 auto;
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 11px;
  padding: 0 2px;
}

.hint {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-3);
  padding: 16px 4px;
}

.empty {
  display: grid;
  place-items: center;
  gap: 8px;
  padding: 48px 12px;
  color: var(--c-text-3);
  font-size: 13px;
}
.et {
  color: var(--c-text-2);
}
.link {
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 12px;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
  min-height: 0;
  overflow: auto;
  flex: 1;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
}
.row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 52px 72px 92px minmax(0, 1.1fr) 72px;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 10px;
  font-size: 12.5px;
  border-bottom: 0.5px solid var(--c-border);
  cursor: default;
}
.row:last-child {
  border-bottom: 0;
}
.row:hover {
  background: var(--c-hover);
}
.row.on {
  background: var(--c-primary-bg);
}
.cell {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cell.name {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--c-text);
}
.cell.kind,
.cell.size,
.cell.time,
.cell.st {
  color: var(--c-text-2);
  font-size: 11.5px;
}
.cell.time,
.cell.size {
  font-variant-numeric: tabular-nums;
}

.tchip {
  display: inline-block;
  font-size: 10px;
  line-height: 1.5;
  padding: 0 6px;
  margin-right: 3px;
  border-radius: var(--r-pill);
  background: var(--c-field);
  color: var(--c-text-2);
  max-width: 110px;
  overflow: hidden;
  text-overflow: ellipsis;
  vertical-align: middle;
}
.tchip.topic {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}
.tchip.user {
  background: transparent;
  border: 1px dashed var(--c-border-strong);
}
.tchip.more {
  background: transparent;
  color: var(--c-text-3);
}

/* 详情条 */
.detail {
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  animation: d-in var(--dur-fast, 120ms) ease both;
}
@keyframes d-in {
  from {
    opacity: 0;
    transform: translateY(3px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.dhead {
  display: flex;
  align-items: center;
  gap: 6px;
}
.dname {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
}
.x {
  border: 0;
  background: transparent;
  color: var(--c-text-3);
  font-size: 14px;
  padding: 0 4px;
}
.x:hover {
  color: var(--c-text);
}
.dsummary {
  margin: 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text);
}
.dsummary.none {
  color: var(--c-text-3);
}
.dtags {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  margin: 0;
}
.dnote {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
}
.dpath {
  margin: 0;
  font-size: 11px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mono {
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
}
.dacts {
  display: flex;
  gap: 8px;
}
.btn {
  border: 0;
  border-radius: var(--r-btn);
  padding: 5px 12px;
  font-size: 12px;
  background: var(--c-field);
  color: var(--c-text);
  font-family: var(--font);
}
.btn:hover {
  color: var(--c-primary);
}

/* 右键菜单 */
.menu {
  position: fixed;
  z-index: 50;
  display: flex;
  flex-direction: column;
  min-width: 160px;
  padding: 4px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: 10px;
  box-shadow: var(--sh-card);
}
.mi {
  border: 0;
  background: transparent;
  text-align: left;
  font-size: 12.5px;
  font-family: var(--font);
  color: var(--c-text);
  padding: 6px 10px;
  border-radius: 7px;
}
.mi:hover {
  background: var(--c-hover);
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
