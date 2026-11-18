<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref } from 'vue'
import { useWizardStore } from '../../stores/wizard'
import type { ProposalTopic } from '../../core/emergence/proposal'
import { countTier } from '../../core/emergence/view'

const props = defineProps<{ topic: ProposalTopic; allowMerge?: boolean }>()
const w = useWizardStore()

const open = ref(false)
const editing = ref(false)
const draft = ref('')
const merging = ref(false)
const confirming = ref(false)
const inputRef = ref<HTMLInputElement | null>(null)
let confirmTimer: number | undefined

const count = computed(() => props.topic.fileIds.length)
const tier = computed(() => countTier(count.value))
const names = computed(() =>
  props.topic.fileIds.map((id) => w.fileIndex[id] ?? '（文件已不在扫描结果里）')
)
const others = computed(
  () => (w.proposalTopics as ProposalTopic[]).filter((t) => t.id !== props.topic.id)
)

async function startEdit() {
  editing.value = true
  draft.value = props.topic.name
  await nextTick()
  inputRef.value?.select()
}

function save() {
  if (!editing.value) return
  w.renameTopic(props.topic.id, draft.value)
  editing.value = false
}

/** 中文输入法选词的回车不算确认，否则打「技术谱系」按回车选字就把分类改没了 */
function onEnter(e: KeyboardEvent) {
  if (e.isComposing) return
  save()
}

function doMerge(toId: string) {
  merging.value = false
  w.mergeTopics(props.topic.id, toId)
}

/** 删除是二级确认：第一次点只是亮起，三秒内再点才真删 */
function del() {
  if (!confirming.value) {
    confirming.value = true
    confirmTimer = window.setTimeout(() => (confirming.value = false), 3000)
    return
  }
  window.clearTimeout(confirmTimer)
  w.removeTopic(props.topic.id)
}

onUnmounted(() => window.clearTimeout(confirmTimer))
</script>

<template>
  <div class="tr" :class="[tier, { open }]">
    <div class="tr-num">{{ count }}</div>

    <div class="tr-main">
      <div class="tr-line">
        <input
          v-if="editing"
          ref="inputRef"
          v-model="draft"
          class="tr-input"
          @keydown.enter="onEnter"
          @keydown.esc="editing = false"
          @blur="save"
        />
        <button v-else class="tr-name" title="点击改名" @click="startEdit">
          {{ topic.name }}
        </button>

        <span v-if="topic.weakReason" class="tr-badge">
          {{ topic.weakReason === 'who' ? '人名/单位' : '仅文种' }}
        </span>

        <button v-if="count > 0" class="tr-more" @click="open = !open">
          {{ open ? '收起' : `查看 ${count} 份文件` }}
        </button>
        <span v-else class="tr-empty">这个分类里没有文件了</span>

        <div class="tr-acts">
          <button @click="startEdit">改名</button>
          <button v-if="allowMerge !== false && others.length" @click="merging = !merging">
            并入
          </button>
          <button class="danger" :class="{ armed: confirming }" @click="del">
            {{ confirming ? '确认删除' : '删除' }}
          </button>
        </div>
      </div>

      <div v-if="merging" class="tr-merge">
        <div class="tr-merge-tip">把「{{ topic.name }}」并进：</div>
        <div class="tr-merge-list">
          <button v-for="o in others" :key="o.id" @click="doMerge(o.id)">
            {{ o.name }}<span>{{ o.fileIds.length }}</span>
          </button>
        </div>
      </div>

      <div v-if="open" class="tr-files">
        <span v-for="(n, i) in names" :key="i" class="tr-file" :title="n">{{ n }}</span>
      </div>
      <div v-else-if="count > 0" class="tr-samples">
        <span v-for="n in names.slice(0, 2)" :key="n" class="tr-samp" :title="n">{{ n }}</span>
        <span v-if="count > 2" class="tr-rest">还有 {{ count - 2 }} 份</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tr {
  display: flex;
  gap: 14px;
  padding: 11px 28px 12px;
  border-bottom: 0.5px solid var(--c-divider);
}
.tr:hover,
.tr:focus-within {
  background: var(--c-hover);
}
.tr-num {
  flex: 0 0 auto;
  width: 34px;
  text-align: right;
  font-weight: 500;
  line-height: 1.25;
  font-variant-numeric: tabular-nums;
}
.tr.major .tr-num {
  font-size: 23px;
  color: var(--c-text);
}
.tr.normal .tr-num {
  font-size: 19px;
  color: var(--c-text);
}
.tr.minor .tr-num {
  font-size: 17px;
  color: var(--c-text-3);
}
.tr-main {
  flex: 1;
  min-width: 0;
}
.tr-line {
  display: flex;
  align-items: center;
  gap: 9px;
  min-height: 26px;
}
.tr-name {
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
  text-align: left;
}
.tr.major .tr-name {
  font-size: 15px;
}
.tr-name:hover {
  background: var(--c-primary-bg);
}
.tr-input {
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  color: var(--c-text);
  padding: 2px 6px;
  border: 0.5px solid var(--c-primary);
  border-radius: var(--r-item);
  background: var(--c-surface);
  min-width: 200px;
  outline: none;
}
.tr-badge {
  flex: 0 0 auto;
  font-size: 11px;
  padding: 1px 7px;
  border-radius: var(--r-pill);
  background: var(--c-hover);
  color: var(--c-text-3);
}
.tr-more {
  flex: 0 0 auto;
  border: none;
  background: transparent;
  padding: 2px 4px;
  font-family: inherit;
  font-size: 12px;
  color: var(--c-primary);
  cursor: pointer;
  border-radius: var(--r-item);
}
.tr-more:hover {
  background: var(--c-primary-bg);
}
.tr-empty {
  font-size: 12px;
  color: var(--c-warning);
}
.tr-acts {
  margin-left: auto;
  flex: 0 0 auto;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity 0.12s;
}
.tr:hover .tr-acts,
.tr:focus-within .tr-acts {
  opacity: 1;
}
.tr-acts button {
  font-family: inherit;
  font-size: 12px;
  padding: 3px 9px;
  border: 0.5px solid var(--c-border-strong);
  border-radius: var(--r-item);
  background: var(--c-surface);
  color: var(--c-text-2);
  cursor: pointer;
}
.tr-acts button:hover {
  color: var(--c-primary);
  border-color: var(--c-primary-glow);
}
.tr-acts button.danger:hover,
.tr-acts button.armed {
  color: var(--c-danger);
  border-color: var(--c-danger);
  background: var(--c-danger-bg);
}
.tr-samples {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 5px;
}
.tr-samp {
  font-size: 12px;
  color: var(--c-text-2);
  background: var(--c-field);
  padding: 2px 8px;
  border-radius: 5px;
  max-width: 300px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tr-rest {
  font-size: 12px;
  color: var(--c-text-3);
  padding: 2px 0;
}
.tr-files {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 5px 18px;
  margin-top: 7px;
  padding: 9px 12px;
  background: var(--c-field);
  border-radius: var(--r-item);
  max-height: 220px;
  overflow-y: auto;
}
.tr-file {
  font-size: 12px;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tr-merge {
  margin-top: 8px;
  padding: 10px 12px;
  background: var(--c-field);
  border-radius: var(--r-item);
}
.tr-merge-tip {
  font-size: 12px;
  color: var(--c-text-2);
  margin-bottom: 7px;
}
.tr-merge-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-height: 150px;
  overflow-y: auto;
}
.tr-merge-list button {
  font-family: inherit;
  font-size: 12px;
  padding: 4px 10px;
  border: 0.5px solid var(--c-border-strong);
  border-radius: var(--r-pill);
  background: var(--c-surface);
  color: var(--c-text);
  cursor: pointer;
}
.tr-merge-list button:hover {
  border-color: var(--c-primary);
  color: var(--c-primary);
}
.tr-merge-list button span {
  margin-left: 5px;
  color: var(--c-text-3);
}
</style>
