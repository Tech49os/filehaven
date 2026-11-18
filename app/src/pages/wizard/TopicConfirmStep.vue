<script setup lang="ts">
import { computed, ref } from 'vue'
import { useWizardStore } from '../../stores/wizard'
import type { ProposalTopic } from '../../core/emergence/proposal'
import TopicRow from './TopicRow.vue'

const w = useWizardStore()
const newName = ref('')
const showWeak = ref(false)
const showUn = ref(false)
const confirmingDrop = ref(false)

const topics = computed(() => w.proposalTopics as ProposalTopic[])
/** 正经分类：名字本身说明了「哪件事」 */
const strong = computed(() => topics.value.filter((t) => !t.weak))
/** 兜底分类：只说人名/单位，或只说文件是什么，默认收起 */
const weak = computed(() => topics.value.filter((t) => t.weak))
const weakFiles = computed(() => weak.value.reduce((n, t) => n + t.fileIds.length, 0))

const assigned = computed(() => strong.value.reduce((n, t) => n + t.fileIds.length, 0))
const ungrouped = computed(() => w.proposal?.ungroupedIds.length ?? 0)
const total = computed(() => assigned.value + weakFiles.value + ungrouped.value)
/** 已归入正经分类的比例 */
const pctOk = computed(() => (total.value ? (assigned.value / total.value) * 100 : 0))
/** 落在兜底类里、等你决定去留的比例 */
const pctMaybe = computed(() => (total.value ? (weakFiles.value / total.value) * 100 : 0))
/** 未归类文件按所在目录分组：这是「还剩什么」的唯一线索 */
const groups = computed(() => w.ungroupedGroups)

function add() {
  const n = newName.value.trim()
  if (!n) return
  w.addTopic(n)
  newName.value = ''
}

/** 中文输入法选词的回车不算提交 */
function onAddEnter(e: KeyboardEvent) {
  if (e.isComposing) return
  add()
}

function dropWeak() {
  if (!confirmingDrop.value) {
    confirmingDrop.value = true
    return
  }
  w.removeTopics(weak.value.map((t) => t.id))
  confirmingDrop.value = false
  showWeak.value = false
}

async function regen() {
  await w.runProposal()
}
</script>

<template>
  <div class="tc">
    <header class="tc-head">
      <div class="tc-title-row">
        <h2 class="tc-title">系统归纳出 {{ strong.length }} 个分类</h2>
        <div class="tc-stats">
          <span><b>{{ assigned }}</b> 份已归入</span>
          <span class="tc-dot">·</span>
          <span :class="{ warn: ungrouped > 0 }"><b>{{ ungrouped }}</b> 份待指定</span>
        </div>
      </div>

      <div
        class="tc-progress"
        :title="`已归入 ${assigned} 份 · 兜底类待定 ${weakFiles} 份 · 未归类 ${ungrouped} 份`"
      >
        <i class="ok" :style="{ width: pctOk + '%' }"></i>
        <i class="maybe" :style="{ width: pctMaybe + '%' }"></i>
      </div>

      <p class="tc-sub">
        点类名可改名，鼠标移到某行上可并入或删除。<strong>改动只存在内存里</strong>，确认后才会真正建目录。
      </p>

      <div class="tc-bar">
        <input
          v-model="newName"
          class="tc-new"
          placeholder="系统漏了哪件事？输入名字，含这个名字的文件会立刻归进来"
          @keydown.enter="onAddEnter"
        />
        <button class="tc-btn ghost sm" :disabled="!newName.trim()" @click="add">新建分类</button>
      </div>
    </header>

    <div class="tc-list">
      <TopicRow v-for="t in strong" :key="t.id" :topic="t" />

      <div v-if="weak.length" class="tc-weak">
        <button class="tc-weak-head" @click="showWeak = !showWeak">
          <span class="tc-caret" :class="{ open: showWeak }">▸</span>
          <span class="tc-weak-text">
            {{ weak.length }} 个分类看不出属于哪件事，共 {{ weakFiles }} 份
          </span>
          <span class="tc-weak-hint">{{ showWeak ? '收起' : '展开看看' }}</span>
        </button>

        <div v-if="showWeak" class="tc-weak-body">
          <p class="tc-weak-desc">
            「演讲稿」「操作手册」这类名字只说明文件性质；「海油发展」这类是单位名——它回答的是「谁在办」，
            不是「哪件事」。它们名下的文件本来就没写清归属，删掉会回到未归类，在下一步手动指定；
            其中有你其实想单独成类的，改个名字留在上面就行。
          </p>
          <div class="tc-weak-ops">
            <button class="tc-btn sm" :class="{ danger: confirmingDrop }" @click="dropWeak">
              {{ confirmingDrop ? `确认删除这 ${weak.length} 个分类` : '全部删除' }}
            </button>
            <span v-if="confirmingDrop" class="tc-hint">再点一次才生效</span>
          </div>
          <TopicRow v-for="t in weak" :key="t.id" :topic="t" />
        </div>
      </div>

      <div v-if="topics.length === 0" class="tc-blank">
        没有归纳出任何分类。可以点下方「重新生成」，或在下一步手动指定。
      </div>

      <div v-if="ungrouped > 0" class="tc-weak tc-un">
        <button class="tc-weak-head" @click="showUn = !showUn">
          <span class="tc-caret" :class="{ open: showUn }">▸</span>
          <span class="tc-weak-text">{{ ungrouped }} 份文件没归类</span>
          <span class="tc-weak-hint">{{ showUn ? '收起' : '看看是哪些' }}</span>
        </button>

        <div v-if="showUn" class="tc-weak-body">
          <p class="tc-weak-desc">
            这些文件名里没写清属于哪件事。按所在文件夹列在下面——如果发现某一堆其实该单独成类，
            在上面输入个名字就能一起归进来；都不值得单独成类的话，留着在下一步手动指定即可。
          </p>
          <div v-for="g in groups" :key="g.folder" class="tc-un-group">
            <div class="tc-un-folder">
              <span class="tc-un-path">{{ g.label }}</span>
              <span class="tc-un-n">{{ g.fileIds.length }} 份</span>
            </div>
            <div class="tc-un-files">
              <span
                v-for="id in g.fileIds.slice(0, 10)"
                :key="id"
                class="tc-un-file"
                :title="w.filePathIndex[id]"
              >
                {{ w.fileIndex[id] ?? '（已不在扫描结果里）' }}
              </span>
              <span v-if="g.fileIds.length > 10" class="tc-un-more">
                还有 {{ g.fileIds.length - 10 }} 份
              </span>
            </div>
          </div>
        </div>
      </div>

      <div class="tc-tail"></div>
    </div>

    <footer class="tc-foot">
      <div class="tc-note">
        {{ ungrouped > 0 ? `未归类的 ${ungrouped} 份文件可在下一步手动指定` : '全部文件都已归入分类' }}
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
}
.tc-head {
  flex: 0 0 auto;
  padding: 18px 28px 12px;
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
  margin-top: 10px;
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
  margin: 9px 0 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--c-text-2);
  max-width: 780px;
}
.tc-sub strong {
  font-weight: 500;
  color: var(--c-text);
}
.tc-bar {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}
.tc-new {
  flex: 1;
  max-width: 480px;
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
.tc-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 2px 0 0;
}
.tc-tail {
  height: 12px;
}
.tc-weak {
  margin: 14px 20px 0;
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  background: var(--c-titlebar);
  overflow: hidden;
}
.tc-weak-head {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 11px 14px;
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: 13px;
  color: var(--c-text-2);
  cursor: pointer;
  text-align: left;
}
.tc-weak-head:hover {
  background: var(--c-hover);
}
.tc-caret {
  display: inline-block;
  font-size: 11px;
  color: var(--c-text-3);
  transition: transform 0.15s;
}
.tc-caret.open {
  transform: rotate(90deg);
}
.tc-weak-text {
  flex: 1;
}
.tc-weak-hint {
  font-size: 12px;
  color: var(--c-primary);
}
.tc-un-group {
  padding: 0 14px 10px;
}
.tc-un-folder {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 4px;
}
.tc-un-path {
  font-size: 12px;
  color: var(--c-text-2);
  font-family: var(--f-mono, ui-monospace, monospace);
}
.tc-un-n {
  font-size: 11px;
  color: var(--c-text-3);
}
.tc-un-files {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.tc-un-file {
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  padding: 2px 8px;
  border-radius: 5px;
  background: var(--c-hover);
  color: var(--c-text-2);
}
.tc-un-more {
  font-size: 11.5px;
  padding: 2px 4px;
  color: var(--c-text-3);
}
.tc-weak-body {
  padding: 0 0 6px;
  border-top: 0.5px solid var(--c-border);
}
/* 嵌在卡片里，行内边距要收窄，否则文字被挤到中间 */
.tc-weak-body :deep(.tr) {
  padding-left: 14px;
  padding-right: 14px;
}
.tc-weak-desc {
  margin: 10px 14px 8px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-3);
  max-width: 720px;
}
.tc-weak-ops {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 14px 8px;
}
.tc-hint {
  font-size: 12px;
  color: var(--c-text-3);
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
  padding: 12px 28px;
  border-top: 0.5px solid var(--c-border);
  background: var(--c-titlebar);
}
.tc-note {
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
  padding: 5px 12px;
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
</style>
