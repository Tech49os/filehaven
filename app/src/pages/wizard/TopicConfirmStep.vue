<script setup lang="ts">
/**
 * 确认分类方案：左树 + 右清单。
 *
 * 左树回答「会建出哪些目录」（大类 → 事项，层级一眼看全），
 * 右清单回答「这个目录里到底装了哪些文件」。
 * 上一版是平铺一列 + 每行展开，45 个类就是 45 行长文，看不出结构也没法比大小。
 *
 * 所有改动都只在内存里，点了「确认并继续」才建目录。
 */
import { computed, nextTick, ref, watch } from 'vue'
import { useWizardStore } from '../../stores/wizard'
import { buildTree } from '../../core/emergence/domain'
import type { ProposalTopic } from '../../core/emergence/proposal'
import TopicTree from './TopicTree.vue'

const w = useWizardStore()

const newName = ref('')
const sel = ref('')
const editing = ref(false)
const draft = ref('')
const moving = ref(false)
const confirmingDrop = ref(false)
let dropTimer: number | undefined

const topics = computed(() => w.proposalTopics as ProposalTopic[])
const strong = computed(() => topics.value.filter((t) => !t.weak))
const weak = computed(() => topics.value.filter((t) => t.weak))
const tree = computed(() => buildTree(strong.value))
const weakFiles = computed(() => weak.value.reduce((n, t) => n + t.fileIds.length, 0))

const assigned = computed(() => strong.value.reduce((n, t) => n + t.fileIds.length, 0))
const ungrouped = computed(() => w.proposal?.ungroupedIds.length ?? 0)
const total = computed(() => assigned.value + weakFiles.value + ungrouped.value)
const pctOk = computed(() => (total.value ? (assigned.value / total.value) * 100 : 0))
const pctMaybe = computed(() => (total.value ? (weakFiles.value / total.value) * 100 : 0))
const groups = computed(() => w.ungroupedGroups)

/** 选中的大类 / 事项 —— 树上的 key 是 `d:名字`、`t:id` */
const curDomain = computed(() =>
  sel.value.startsWith('d:') ? tree.value.domains.find((d) => d.name === sel.value.slice(2)) ?? null : null
)
const curTopic = computed(() => {
  if (!sel.value.startsWith('t:')) return null
  const id = sel.value.slice(2)
  return topics.value.find((t) => t.id === id) ?? null
})

/** 右侧标题（域名 / 事项名 / 待定 / 未归类） */
const title = computed(() => {
  if (curDomain.value) return curDomain.value.name
  if (curTopic.value) return curTopic.value.name
  if (sel.value === 'weak') return '待定'
  return '未归类'
})

/** 右清单的文件行：domain 视图按事项分组，topic 视图单组 */
interface FileSec {
  head: string
  count: number
  ids: string[]
}
const sections = computed<FileSec[]>(() => {
  if (curDomain.value) {
    return curDomain.value.topics.map((t) => ({
      head: t.name,
      count: t.fileIds.length,
      ids: t.fileIds,
    }))
  }
  if (curTopic.value) {
    return [{ head: '', count: curTopic.value.fileIds.length, ids: curTopic.value.fileIds }]
  }
  if (sel.value === 'weak') {
    // 待定：按事项分组，让用户看清「这堆东西分别是谁的」
    return weak.value.map((t) => ({ head: t.name, count: t.fileIds.length, ids: t.fileIds }))
  }
  return []
})

const fileCount = computed(() => sections.value.reduce((n, s) => n + s.ids.length, 0))
/** 工具栏右上角的份数：未归类面板走的是目录分组，得用未归类的总数 */
const shownCount = computed(() => (sel.value === 'un' ? ungrouped.value : fileCount.value))

const domainHint = computed(() => {
  if (w.domainSource === 'llm')
    return `本地模型按你电脑上文件的方向分了 ${tree.value.domains.length} 个大类。大类可改名、可解散，事项可挪到别的大类。`
  if (w.domainSource === 'rule')
    return '没连上模型，按事项名的共同词粗略分了大类（可能不如模型准）。可以自己改名或解散。'
  return '没有分成大类，所有事项都放在归档根目录的第一层。'
})

/** 选中的节点被删掉 / 重新生成后自动落到一个还有内容的位置 */
watch(
  [tree, weak, ungrouped, () => w.proposal],
  () => {
    if (sel.value === 'un' && ungrouped.value > 0) return
    if (sel.value === 'weak' && weak.value.length > 0) return
    if (curDomain.value) return
    if (curTopic.value) return
    const firstDomain = tree.value.domains[0]
    if (firstDomain) sel.value = `d:${firstDomain.name}`
    else if (tree.value.loose[0]) sel.value = `t:${tree.value.loose[0].id}`
    else if (weak.value.length) sel.value = 'weak'
    else sel.value = 'un'
  },
  { immediate: true }
)

function add() {
  const n = newName.value.trim()
  if (!n) return
  w.addTopic(n, curDomain.value?.name)
  newName.value = ''
}

/** 中文输入法选词的回车不算提交 */
function onAddEnter(e: KeyboardEvent) {
  if (e.isComposing) return
  add()
}

function startEdit() {
  editing.value = true
  draft.value = title.value
  moving.value = false
  void nextTick(() => {
    const el = document.querySelector<HTMLInputElement>('.tc-edit')
    el?.select()
  })
}

function saveEdit() {
  if (!editing.value) return
  editing.value = false
  const name = draft.value.trim()
  if (!name) return
  if (curDomain.value) {
    w.renameDomain(curDomain.value.name, name)
    sel.value = `d:${name}`
  } else if (curTopic.value) {
    w.renameTopic(curTopic.value.id, name)
  }
}

function onEditEnter(e: KeyboardEvent) {
  if (e.isComposing) return
  saveEdit()
}

function dissolve() {
  if (!curDomain.value) return
  const name = curDomain.value.name
  w.dissolveDomain(name)
  sel.value = ''
}

function removeTopic() {
  if (!curTopic.value) return
  w.removeTopic(curTopic.value.id)
}

function moveTo(domain: string | null) {
  if (!curTopic.value) return
  w.moveTopicToDomain(curTopic.value.id, domain)
  moving.value = false
  sel.value = curTopic.value.domain ? `d:${curTopic.value.domain}` : ''
}

/** 删除待定类是二级确认：第一次点只是亮起，三秒内再点才真删 */
function dropWeak() {
  if (!confirmingDrop.value) {
    confirmingDrop.value = true
    dropTimer = window.setTimeout(() => (confirmingDrop.value = false), 3000)
    return
  }
  window.clearTimeout(dropTimer)
  w.removeTopics(weak.value.map((t) => t.id))
  confirmingDrop.value = false
  sel.value = ''
}

async function regen() {
  await w.runProposal()
  sel.value = ''
}
</script>

<template>
  <div class="tc">
    <header class="tc-head">
      <div class="tc-title-row">
        <h2 class="tc-title">
          系统归纳出 {{ tree.domains.length ? `${tree.domains.length} 个大类、` : '' }}{{ strong.length }} 个事项
        </h2>
        <div class="tc-stats">
          <span><b>{{ assigned }}</b> 份已归入</span>
          <span class="tc-dot">·</span>
          <span :class="{ warn: ungrouped > 0 }"><b>{{ ungrouped }}</b> 份待指定</span>
        </div>
      </div>

      <div
        class="tc-progress"
        :title="`已归入 ${assigned} 份 · 待定类 ${weakFiles} 份 · 未归类 ${ungrouped} 份`"
      >
        <i class="ok" :style="{ width: pctOk + '%' }"></i>
        <i class="maybe" :style="{ width: pctMaybe + '%' }"></i>
      </div>

      <p class="tc-sub">{{ domainHint }}<strong>改动只存在内存里</strong>，确认后才会真正建目录。</p>

      <div class="tc-bar">
        <input
          v-model="newName"
          class="tc-new"
          :placeholder="
            curDomain
              ? `在「${curDomain.name}」里补一个事项：输入名字，含这个名字的文件会立刻归进来`
              : '系统漏了哪件事？输入名字，含这个名字的文件会立刻归进来'
          "
          @keydown.enter="onAddEnter"
        />
        <button class="tc-btn ghost sm" :disabled="!newName.trim()" @click="add">新建事项</button>
      </div>
    </header>

    <div class="tc-main">
      <aside class="tc-side">
        <TopicTree
          :tree="tree"
          :weak-count="weak.length"
          :weak-files="weakFiles"
          :ungrouped="ungrouped"
          :selected="sel"
          @select="sel = $event"
        />
      </aside>

      <section class="tc-panel">
        <!-- 工具栏：针对当前节点能做的事 -->
        <div class="tc-tools">
          <template v-if="editing">
            <input
              v-model="draft"
              class="tc-edit"
              @keydown.enter="onEditEnter"
              @keydown.esc="editing = false"
              @blur="saveEdit"
            />
            <span class="tc-hint">回车确认</span>
          </template>
          <template v-else>
            <h3 class="tc-h3" :title="curTopic || curDomain ? '点击改名' : ''">
              <button
                v-if="curTopic || curDomain"
                class="tc-name-btn"
                @click="startEdit"
              >
                {{ title }}
              </button>
              <span v-else>{{ title }}</span>
            </h3>
            <span v-if="curDomain" class="tc-tag">
              {{ curDomain.topics.length }} 个事项 · 建成一层目录
            </span>
            <span v-if="curTopic?.weakReason" class="tc-tag">
              {{ curTopic.weakReason === 'who' ? '人名 / 单位名' : '只说明文件性质' }}
            </span>
            <span class="tc-count">{{ shownCount }} 份文件</span>

            <div class="tc-acts">
              <template v-if="curDomain">
                <button class="tc-btn sm" @click="startEdit">改名</button>
                <button class="tc-btn sm" @click="dissolve">解散大类</button>
              </template>
              <template v-else-if="curTopic">
                <button class="tc-btn sm" @click="startEdit">改名</button>
                <div class="tc-move">
                  <button class="tc-btn sm" @click="moving = !moving">移到…</button>
                  <div v-if="moving" class="tc-move-pop">
                    <button @click="moveTo(null)">放到第一层</button>
                    <button
                      v-for="d in w.domainNames.filter((x) => x !== curTopic?.domain)"
                      :key="d"
                      @click="moveTo(d)"
                    >
                      {{ d }}
                    </button>
                  </div>
                </div>
                <button class="tc-btn sm danger" @click="removeTopic">删除</button>
              </template>
              <template v-else-if="sel === 'weak'">
                <button class="tc-btn sm" :class="{ danger: confirmingDrop }" @click="dropWeak">
                  {{ confirmingDrop ? `确认删除这 ${weak.length} 个` : '全部删除' }}
                </button>
              </template>
            </div>
          </template>
        </div>

        <div class="tc-scroll">
          <!-- 待定类：先说明它是什么，再列文件 -->
          <p v-if="sel === 'weak'" class="tc-note">
            「演讲稿」「操作手册」这类名字只说明文件性质；「海油发展」这类是单位名——它回答的是「谁在办」，
            不是「哪件事」。它们的文件本来就没写清归属，删掉会回到未归类；其中有你想单独成类的，改个名字就行。
          </p>

          <!-- 未归类：按来源目录分组，这是「还剩什么」的唯一线索 -->
          <template v-if="sel === 'un'">
            <p v-if="ungrouped" class="tc-note">
              这些文件名里没写清属于哪件事，按来源文件夹列在下面。发现某一堆其实该单独成类，
              在上面输入个名字就能一起归进来（输入时选中的大类决定了它建在哪）。
            </p>
            <div v-for="g in groups" :key="g.folder" class="tc-sec">
              <div class="tc-sec-h">
                <span class="tc-folder">{{ g.label }}</span>
                <span class="tc-sec-n">{{ g.fileIds.length }} 份</span>
              </div>
              <div class="tc-files">
                <div v-for="id in g.fileIds" :key="id" class="tc-file">
                  <span class="tc-fn">{{ w.fileIndex[id] ?? '（已不在扫描结果里）' }}</span>
                  <span class="tc-fp">{{ w.filePathIndex[id] }}</span>
                </div>
              </div>
            </div>
            <div v-if="!ungrouped" class="tc-blank">全部文件都已归入分类。</div>
          </template>

          <!-- 大类 / 事项 / 待定：文件清单 -->
          <template v-else>
            <div v-if="!fileCount" class="tc-blank">这个分类里没有文件了。</div>
            <div v-for="s in sections" :key="s.head" class="tc-sec">
              <div v-if="s.head" class="tc-sec-h">
                <span class="tc-sec-t">{{ s.head }}</span>
                <span class="tc-sec-n">{{ s.count }} 份</span>
              </div>
              <div class="tc-files">
                <div v-for="id in s.ids" :key="id" class="tc-file">
                  <span class="tc-fn">{{ w.fileIndex[id] ?? '（已不在扫描结果里）' }}</span>
                  <span class="tc-fp">{{ w.filePathIndex[id] }}</span>
                </div>
              </div>
            </div>
          </template>
        </div>
      </section>
    </div>

    <footer class="tc-foot">
      <div class="tc-foot-note">
        {{
          ungrouped > 0
            ? `未归类的 ${ungrouped} 份文件会留在原处，可在下一步手动指定`
            : '全部文件都已归入分类'
        }}
      </div>
      <div class="tc-btns">
        <button class="tc-btn ghost" @click="regen">重新生成</button>
        <button class="tc-btn primary" :disabled="strong.length === 0" @click="w.confirmProposal()">
          确认并继续
        </button>
      </div>
    </footer>
  </div>
</template>

<style scoped>
.tc {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}
.tc-head {
  flex: 0 0 auto;
  padding: 16px 24px 12px;
  border-bottom: 0.5px solid var(--c-border);
}
.tc-title-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  flex-wrap: wrap;
}
.tc-title {
  margin: 0;
  font-size: 17px;
  font-weight: 500;
  color: var(--c-text);
}
.tc-stats {
  font-size: 12px;
  color: var(--c-text-2);
}
.tc-stats b {
  font-weight: 500;
  color: var(--c-text);
}
.tc-stats .warn b {
  color: var(--c-warning);
}
.tc-dot {
  margin: 0 5px;
  color: var(--c-text-3);
}
.tc-progress {
  display: flex;
  height: 3px;
  margin-top: 9px;
  border-radius: 2px;
  background: var(--c-hover);
  overflow: hidden;
}
.tc-progress i {
  display: block;
  height: 100%;
  transition: width 0.25s;
}
.tc-progress i.ok {
  background: var(--c-primary);
}
.tc-progress i.maybe {
  background: var(--c-text-3);
}
.tc-sub {
  margin: 8px 0 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--c-text-2);
  max-width: 860px;
}
.tc-sub strong {
  font-weight: 500;
  color: var(--c-text);
}
.tc-bar {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}
.tc-new {
  flex: 1;
  max-width: 520px;
  font-family: inherit;
  font-size: 13px;
  padding: 7px 12px;
  border: 0.5px solid var(--c-border-strong);
  border-radius: var(--r-field);
  background: var(--c-field);
  color: var(--c-text);
  outline: none;
}
.tc-new:focus {
  border-color: var(--c-primary);
  background: var(--c-surface);
}
.tc-new::placeholder {
  color: var(--c-text-3);
}

/* 左树 + 右清单 */
.tc-main {
  flex: 1;
  min-height: 0;
  display: grid;
  /* minmax(0,1fr)：内容再多也不许把这一行撑出窗口高度，否则整页会跟着长 */
  grid-template-columns: 250px minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);
  overflow: hidden;
}
.tc-side {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  border-right: 0.5px solid var(--c-border);
  background: var(--c-titlebar);
}
.tc-panel {
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.tc-tools {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 20px;
  border-bottom: 0.5px solid var(--c-divider);
  min-height: 44px;
  flex-wrap: wrap;
}
.tc-h3 {
  margin: 0;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text);
}
.tc-name-btn {
  border: none;
  background: transparent;
  padding: 1px 4px;
  margin-left: -4px;
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text);
  cursor: text;
  border-radius: var(--r-item);
}
.tc-name-btn:hover {
  background: var(--c-primary-bg);
}
.tc-edit {
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text);
  padding: 3px 8px;
  border: 0.5px solid var(--c-primary);
  border-radius: var(--r-item);
  background: var(--c-surface);
  min-width: 200px;
  outline: none;
}
.tc-tag {
  font-size: 11px;
  padding: 1px 7px;
  border-radius: var(--r-pill);
  background: var(--c-hover);
  color: var(--c-text-3);
}
.tc-count {
  font-size: 12px;
  color: var(--c-text-2);
  font-variant-numeric: tabular-nums;
}
.tc-acts {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 6px;
}
.tc-move {
  position: relative;
}
.tc-move-pop {
  position: absolute;
  right: 0;
  top: calc(100% + 4px);
  z-index: 5;
  min-width: 140px;
  max-height: 240px;
  overflow-y: auto;
  padding: 4px;
  border: 0.5px solid var(--c-border-strong);
  border-radius: var(--r-card);
  background: var(--c-surface);
  box-shadow: var(--sh-card);
}
.tc-move-pop button {
  display: block;
  width: 100%;
  text-align: left;
  font-family: inherit;
  font-size: 12px;
  padding: 6px 8px;
  border: none;
  border-radius: var(--r-item);
  background: transparent;
  color: var(--c-text);
  cursor: pointer;
}
.tc-move-pop button:hover {
  background: var(--c-hover);
  color: var(--c-primary);
}

.tc-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 12px 20px 20px;
}
.tc-note {
  margin: 0 0 12px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-3);
  max-width: 760px;
}
.tc-sec {
  margin-bottom: 14px;
}
.tc-sec-h {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 4px 0 5px;
  border-bottom: 0.5px solid var(--c-divider);
  margin-bottom: 4px;
}
.tc-sec-t {
  font-size: 12.5px;
  font-weight: 500;
  color: var(--c-text);
}
.tc-folder {
  font-size: 12px;
  color: var(--c-text-2);
  font-family: ui-monospace, Menlo, Consolas, monospace;
}
.tc-sec-n {
  font-size: 11.5px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}
.tc-files {
  display: flex;
  flex-direction: column;
}
.tc-file {
  display: flex;
  gap: 12px;
  align-items: baseline;
  padding: 3px 4px;
  border-radius: 5px;
}
.tc-file:hover {
  background: var(--c-hover);
}
.tc-fn {
  flex: 1;
  min-width: 0;
  font-size: 12.5px;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-fp {
  flex: 0 1 40%;
  min-width: 0;
  font-size: 11px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: right;
  direction: rtl;
}
.tc-blank {
  padding: 40px 0;
  text-align: center;
  color: var(--c-text-3);
  font-size: 13px;
}

.tc-foot {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 11px 24px;
  border-top: 0.5px solid var(--c-border);
  background: var(--c-titlebar);
}
.tc-foot-note {
  font-size: 12px;
  color: var(--c-text-2);
}
.tc-btns {
  margin-left: auto;
  display: flex;
  gap: 10px;
}
.tc-btn {
  font-family: inherit;
  font-size: 13px;
  padding: 7px 18px;
  border-radius: var(--r-btn);
  cursor: pointer;
  border: 0.5px solid var(--c-border-strong);
  background: var(--c-surface);
  color: var(--c-text);
}
.tc-btn.sm {
  font-size: 12px;
  padding: 4px 11px;
  white-space: nowrap;
}
.tc-btn.primary {
  background: var(--c-primary);
  border-color: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.tc-btn.primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
  box-shadow: none;
}
.tc-btn.ghost:hover {
  background: var(--c-hover);
}
.tc-btn.danger {
  color: var(--c-danger);
  border-color: var(--c-danger);
  background: var(--c-danger-bg);
}
.tc-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.tc-hint {
  font-size: 12px;
  color: var(--c-text-3);
}
</style>
