<script setup lang="ts">
import { computed, ref } from 'vue'
import { useWizardStore } from '../../stores/wizard'
import type { ProposalTopic } from '../../core/emergence/proposal'

const w = useWizardStore()
const editing = ref<string | null>(null)
const draft = ref('')
const newName = ref('')
const showWeak = ref(false)

const topics = computed(() => w.proposalTopics as ProposalTopic[])
/** 正经事项：名字本身说明了「哪件事」 */
const strong = computed(() => topics.value.filter((t) => !t.weak))
/** 兜底类：名字只是文种（演讲稿 / 操作手册 / 进展汇报），建议删掉 */
const weak = computed(() => topics.value.filter((t) => t.weak))
const weakFiles = computed(() => weak.value.reduce((n, t) => n + t.fileIds.length, 0))

const assigned = computed(() => topics.value.reduce((n, t) => n + t.fileIds.length, 0))
const ungrouped = computed(() => w.proposal?.ungroupedIds.length ?? 0)

function startEdit(t: ProposalTopic) {
  editing.value = t.id
  draft.value = t.name
}

function save(id: string) {
  if (editing.value !== id) return
  w.renameTopic(id, draft.value)
  editing.value = null
}

function onMerge(id: string, e: Event) {
  const el = e.target as HTMLSelectElement
  const to = el.value
  el.value = ''
  if (to) w.mergeTopics(id, to)
}

function add() {
  const n = newName.value.trim()
  if (!n) return
  w.addTopic(n)
  newName.value = ''
}

function dropWeak() {
  w.removeTopics(weak.value.map((t) => t.id))
  showWeak.value = false
}

async function regen() {
  await w.runProposal()
}
</script>

<template>
  <div class="tc">
    <header class="tc-head">
      <div class="tc-title">
        系统归纳出 {{ strong.length }} 个分类
        <span v-if="weak.length" class="tc-weak-tag">+ {{ weak.length }} 个仅文种</span>
      </div>
      <p class="tc-sub">
        {{ assigned }} 份已归入，{{ ungrouped }} 份未能归类。改名字、删掉不需要的、把重复的并到一起，
        改完会自动重算归属。确认之后才会真正建目录。
      </p>
      <div class="tc-bar">
        <input
          v-model="newName"
          class="tc-new"
          placeholder="系统漏了哪件事？输入名字回车，含这个名字的文件会立刻归进来"
          @keyup.enter="add"
        />
        <button class="tc-btn ghost sm" :disabled="!newName.trim()" @click="add">新建分类</button>
      </div>
    </header>

    <div class="tc-list">
      <div v-for="t in strong" :key="t.id" class="tc-item">
        <div class="tc-row">
          <input
            v-if="editing === t.id"
            v-model="draft"
            class="tc-input"
            autofocus
            @keyup.enter="save(t.id)"
            @blur="save(t.id)"
          />
          <button v-else class="tc-name" title="点一下改名" @click="startEdit(t)">
            {{ t.name }}
          </button>
          <span class="tc-count" :class="{ zero: t.fileIds.length === 0 }">
            {{ t.fileIds.length }} 份
          </span>
          <div class="tc-actions">
            <select v-if="topics.length > 1" class="tc-sel" @change="onMerge(t.id, $event)">
              <option value="">并入…</option>
              <option v-for="o in topics.filter((x) => x.id !== t.id)" :key="o.id" :value="o.id">
                {{ o.name }}
              </option>
            </select>
            <button class="tc-del" @click="w.removeTopic(t.id)">删除</button>
          </div>
        </div>
        <div v-if="t.samples.length" class="tc-samples">
          <span v-for="s in t.samples.slice(0, 3)" :key="s" class="tc-sample">{{ s }}</span>
        </div>
        <div v-else class="tc-samples">
          <span class="tc-empty">这个分类下已经没有文件，可以删掉</span>
        </div>
      </div>

      <div v-if="weak.length" class="tc-weak">
        <button class="tc-weak-head" @click="showWeak = !showWeak">
          <span class="tc-caret" :class="{ open: showWeak }">▸</span>
          另有 {{ weak.length }} 个「只说了文件是什么、没说属于哪件事」的分类，
          共 {{ weakFiles }} 份
          <span class="tc-weak-hint">{{ showWeak ? '收起' : '展开删掉' }}</span>
        </button>
        <div v-if="showWeak" class="tc-weak-body">
          <p class="tc-weak-desc">
            像「演讲稿」「操作手册」这种名字，只说明文档性质。它们的文件本来就没写清属于哪件事，
            删掉后这些文件会回到未归类，在下一步手动指定。
          </p>
          <button class="tc-btn ghost sm" @click="dropWeak">全部删除</button>
          <div v-for="t in weak" :key="t.id" class="tc-item">
            <div class="tc-row">
              <button class="tc-name" @click="startEdit(t)">{{ t.name }}</button>
              <span class="tc-count">{{ t.fileIds.length }} 份</span>
              <div class="tc-actions">
                <button class="tc-del" @click="w.removeTopic(t.id)">删除</button>
              </div>
            </div>
            <div v-if="t.samples.length" class="tc-samples">
              <span v-for="s in t.samples.slice(0, 2)" :key="s" class="tc-sample">{{ s }}</span>
            </div>
          </div>
        </div>
      </div>

      <div v-if="topics.length === 0" class="tc-blank">
        没有归纳出任何分类。可以点下方「重新生成」，或在下一步手动指定。
      </div>
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
  padding: 20px 28px 12px;
}
.tc-title {
  font-size: 18px;
  font-weight: 500;
  color: var(--c-text);
}
.tc-weak-tag {
  font-size: 12px;
  font-weight: 400;
  color: var(--c-text-3);
  margin-left: 6px;
}
.tc-sub {
  margin: 8px 0 0;
  font-size: 13px;
  line-height: 1.7;
  color: var(--c-text-2);
  max-width: 760px;
}
.tc-bar {
  display: flex;
  gap: 8px;
  margin-top: 14px;
}
.tc-new {
  flex: 1;
  max-width: 520px;
  font-size: 13px;
  padding: 7px 12px;
  border: 0.5px solid var(--c-border);
  border-radius: 8px;
  background: var(--c-surface);
  color: var(--c-text);
}
.tc-new::placeholder {
  color: var(--c-text-3);
}
.tc-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 28px 16px;
}
.tc-item {
  border-bottom: 0.5px solid var(--c-border);
  padding: 12px 0;
}
.tc-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.tc-name {
  border: none;
  background: transparent;
  padding: 2px 4px;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text);
  cursor: text;
  border-radius: 4px;
  text-align: left;
}
.tc-name:hover {
  background: var(--c-hover);
}
.tc-input {
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text);
  padding: 2px 6px;
  border: 0.5px solid var(--c-primary);
  border-radius: 4px;
  background: var(--c-surface);
  min-width: 180px;
}
.tc-count {
  font-size: 12px;
  color: var(--c-text-2);
  flex: 0 0 auto;
}
.tc-count.zero {
  color: var(--c-warning);
}
.tc-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 0 0 auto;
}
.tc-sel {
  font-size: 12px;
  padding: 3px 6px;
  border: 0.5px solid var(--c-border);
  border-radius: 6px;
  background: var(--c-surface);
  color: var(--c-text-2);
  cursor: pointer;
}
.tc-del {
  font-size: 12px;
  padding: 3px 10px;
  border: 0.5px solid var(--c-border);
  border-radius: 6px;
  background: transparent;
  color: var(--c-text-2);
  cursor: pointer;
}
.tc-del:hover {
  border-color: var(--c-warning);
  color: var(--c-warning);
}
.tc-samples {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 6px;
  padding-left: 4px;
}
.tc-sample {
  font-size: 12px;
  color: var(--c-text-3);
  background: var(--c-hover);
  padding: 2px 8px;
  border-radius: 4px;
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-empty {
  font-size: 12px;
  color: var(--c-text-3);
  font-style: italic;
}
.tc-weak {
  margin-top: 18px;
  border: 0.5px solid var(--c-border);
  border-radius: 10px;
  padding: 4px 12px 8px;
  background: var(--c-titlebar);
}
.tc-weak-head {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 0;
  border: none;
  background: transparent;
  font-size: 13px;
  color: var(--c-text-2);
  cursor: pointer;
  text-align: left;
}
.tc-caret {
  display: inline-block;
  transition: transform 0.15s;
  color: var(--c-text-3);
}
.tc-caret.open {
  transform: rotate(90deg);
}
.tc-weak-hint {
  margin-left: auto;
  font-size: 12px;
  color: var(--c-primary);
}
.tc-weak-body {
  padding-bottom: 4px;
}
.tc-weak-desc {
  margin: 0 0 10px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--c-text-3);
  max-width: 680px;
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
  padding: 14px 28px;
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
  font-size: 13px;
  padding: 7px 18px;
  border-radius: 8px;
  cursor: pointer;
  border: 0.5px solid var(--c-border);
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
}
.tc-btn.primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.tc-btn.ghost:hover {
  background: var(--c-hover);
}
.tc-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
