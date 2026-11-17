<script setup lang="ts">
import { computed, ref } from 'vue'
import { useWizardStore } from '../../stores/wizard'
import type { ProposalTopic } from '../../core/emergence/proposal'

const w = useWizardStore()
const editing = ref<string | null>(null)
const draft = ref('')

const topics = computed(() => w.proposalTopics as ProposalTopic[])
const total = computed(() => w.proposal?.ungroupedIds.length ?? 0)
const assigned = computed(() => {
  const sum = topics.value.reduce((n, t) => n + t.fileIds.length, 0)
  return sum
})
const ungrouped = computed(() => total.value)

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

async function regen() {
  await w.runProposal()
}
</script>

<template>
  <div class="tc">
    <header class="tc-head">
      <div class="tc-title">系统归纳出 {{ topics.length }} 个分类</div>
      <p class="tc-sub">
        {{ assigned }} 份已归入，{{ ungrouped }} 份未能归类。改名字、删掉不需要的、把重复的并到一起，
        改完会自动重算归属。确认之后才会真正建目录。
      </p>
    </header>

    <div class="tc-list">
      <div v-for="t in topics" :key="t.id" class="tc-item">
        <div class="tc-row">
          <input
            v-if="editing === t.id"
            v-model="draft"
            class="tc-input"
            autofocus
            @keyup.enter="save(t.id)"
            @blur="save(t.id)"
          />
          <button v-else class="tc-name" :title="'点一下改名'" @click="startEdit(t)">
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
        <button class="tc-btn primary" :disabled="topics.length === 0" @click="w.confirmProposal()">
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
.tc-sub {
  margin: 8px 0 0;
  font-size: 13px;
  line-height: 1.7;
  color: var(--c-text-2);
  max-width: 760px;
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
</style>
