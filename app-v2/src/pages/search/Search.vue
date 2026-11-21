<script setup lang="ts">
/**
 * 高级筛选页（P3 T3.4）—— 检索三入口的第二张皮。
 *
 * 纪律：不自己写 SQL、不在前端过滤结果 —— 组合条件全部交给共用查询层
 * `search_files`（T3.1），与 Ctrl+K、目录内筛选同一套语义。
 * URL query 带条件（#/search?tag=3&q=x），面板与标签 chip 跳转都能预置。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppIcon from '../../components/AppIcon.vue'
import FileList from '../../components/FileList.vue'
import { livePath } from '../../core/types'
import type { FileKind, TagCount } from '../../core/types'
import * as ipc from '../../core/ipc'

const route = useRoute()
const router = useRouter()

const q = ref('')
const tagIds = ref<number[]>([])
const tagMode = ref<'and' | 'or'>('and')
const kinds = ref<FileKind[]>([])
const scope = ref<'archive' | 'pool' | 'parked' | 'all'>('all')
const from = ref('')
const to = ref('')

const items = ref<any[]>([])
const total = ref(0)
const loading = ref(false)
const searched = ref(false)
const tagList = ref<TagCount[] | null>(null)
const openPathErr = ref('')

const KINDS: { v: FileKind; label: string }[] = [
  { v: 'word', label: '文档' },
  { v: 'excel', label: '表格' },
  { v: 'ppt', label: '演示' },
  { v: 'pdf', label: 'PDF' },
  { v: 'image', label: '图片' },
  { v: 'diagram', label: '图表' },
  { v: 'other', label: '其他' },
]

const condCount = computed(
  () =>
    (q.value.trim() ? 1 : 0) +
    tagIds.value.length +
    kinds.value.length +
    (from.value ? 1 : 0) +
    (to.value ? 1 : 0),
)

function toggle<T>(arr: T[], v: T) {
  const i = arr.indexOf(v)
  if (i >= 0) arr.splice(i, 1)
  else arr.push(v)
}

async function run() {
  loading.value = true
  try {
    const res = await ipc.searchFiles({
      q: q.value.trim(),
      tags: tagIds.value,
      tagMode: tagMode.value,
      kinds: kinds.value,
      from: from.value ? new Date(from.value).getTime() : undefined,
      to: to.value ? new Date(to.value).getTime() + 86_399_000 : undefined,
      scope: scope.value,
      limit: 200,
    })
    items.value = res.items
    total.value = res.total
    searched.value = true
  } finally {
    loading.value = false
  }
}

/** URL query → 条件（#/search?tag=3&q=x），面板与标签 chip 跳转的预置入口 */
function applyQuery() {
  const t = route.query.tag
  if (typeof t === 'string' && /^\d+$/.test(t)) tagIds.value = [Number(t)]
  else tagIds.value = []
  const qs = route.query.q
  q.value = typeof qs === 'string' ? qs : ''
}

function reset() {
  q.value = ''
  tagIds.value = []
  kinds.value = []
  from.value = ''
  to.value = ''
  scope.value = 'all'
  void run()
}

async function openRow(row: any) {
  openPathErr.value = ''
  try {
    await ipc.openPath(livePath(row))
  } catch (e) {
    openPathErr.value = String(e)
  }
}

async function loadTags() {
  try {
    tagList.value = await ipc.listTags()
  } catch {
    tagList.value = []
  }
}

onMounted(() => {
  applyQuery()
  void loadTags()
  void run()
})

// 面板 / 标签 chip 在页面上时跳转 → 条件跟着变
watch(
  () => route.query,
  () => {
    if (route.path === '/search') {
      applyQuery()
      void run()
    }
  },
)

/** 切了条件就重搜（复选、模式、范围都监听）
 *  tagIds 必须在列：e2e-p3 抓到过「点 chip 只亮不重搜」——真 bug，不是测试问题 */
watch([tagMode, kinds, scope, tagIds], () => void run(), { deep: true })
</script>

<template>
  <div class="search">
    <header class="top">
      <input
        v-model="q"
        class="kw"
        type="text"
        placeholder="关键词：文件名或路径…"
        spellcheck="false"
        @keydown.enter.prevent="run"
      />
      <button class="btn primary" @click="run">搜索</button>
    </header>

    <p class="condbar">
      已选 <b>{{ condCount }}</b> 个条件
      <button class="clear" @click="reset">清空</button>
    </p>

    <div class="body">
      <aside class="filters">
        <section v-if="tagList !== null && tagList.length > 0" class="group">
          <h3>标签</h3>
          <div class="mode">
            <button class="m" :class="{ on: tagMode === 'and' }" @click="tagMode = 'and'">同时具备</button>
            <button class="m" :class="{ on: tagMode === 'or' }" @click="tagMode = 'or'">任一</button>
          </div>
          <div class="chips">
            <button
              v-for="t in tagList"
              :key="t.id"
              class="fchip"
              :class="{ on: tagIds.includes(t.id), user: t.kind === 'user' }"
              @click="toggle(tagIds, t.id)"
            >{{ t.name }}<span class="fc">{{ t.fileCount }}</span></button>
          </div>
        </section>

        <section class="group">
          <h3>类型</h3>
          <div class="chips">
            <button
              v-for="k in KINDS"
              :key="k.v"
              class="fchip"
              :class="{ on: kinds.includes(k.v) }"
              @click="toggle(kinds, k.v)"
            >{{ k.label }}</button>
          </div>
        </section>

        <section class="group">
          <h3>时间</h3>
          <div class="dates">
            <input v-model="from" type="date" @change="run" />
            <span class="sep">至</span>
            <input v-model="to" type="date" @change="run" />
          </div>
        </section>

        <section class="group">
          <h3>范围</h3>
          <div class="chips">
            <button class="fchip" :class="{ on: scope === 'all' }" @click="scope = 'all'">全部</button>
            <button class="fchip" :class="{ on: scope === 'archive' }" @click="scope = 'archive'">归档区</button>
            <button class="fchip" :class="{ on: scope === 'pool' }" @click="scope = 'pool'">整理池</button>
            <button class="fchip" :class="{ on: scope === 'parked' }" @click="scope = 'parked'">待定区</button>
          </div>
        </section>
      </aside>

      <div class="results">
        <FileList
          :rows="items"
          :loading="loading"
          @open="openRow"
          @activate="() => {}"
        >
          <template #empty>
            <AppIcon name="info" :size="24" />
            <span>{{ searched ? '没有匹配的文件' : '设好条件点「搜索」' }}</span>
          </template>
        </FileList>
        <p class="totalbar" v-if="searched && !loading">共 {{ total }} 份</p>
        <p v-if="openPathErr" class="err" role="alert">{{ openPathErr }}</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.search {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
  min-height: 0;
}
.top {
  display: flex;
  gap: 8px;
}
.kw {
  flex: 1;
  min-width: 0;
  height: 34px;
  padding: 0 12px;
  border-radius: 9px;
  border: 1px solid var(--c-border);
  background: var(--c-surface);
  font-size: 13px;
  font-family: var(--font);
  color: var(--c-text);
  outline: none;
}
.kw:focus {
  border-color: var(--c-primary);
  box-shadow: 0 0 0 3px var(--c-primary-glow);
}
.btn {
  border: 0;
  border-radius: var(--r-btn);
  padding: 0 16px;
  font-size: 13px;
  background: var(--c-field);
  color: var(--c-text);
  font-family: var(--font);
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
}

.condbar {
  margin: 0;
  font-size: 11.5px;
  color: var(--c-text-3);
  display: flex;
  align-items: center;
  gap: 8px;
}
.clear {
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 11px;
  padding: 0;
  text-decoration: underline;
  text-underline-offset: 2px;
}

.body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 12px;
}
.filters {
  flex: 0 0 220px;
  width: 220px;
  overflow: auto;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.group h3 {
  margin: 0 0 6px;
  font-size: 11px;
  font-weight: 600;
  color: var(--c-text-3);
}
.mode {
  display: flex;
  gap: 4px;
  margin-bottom: 6px;
}
.m {
  border: 1px solid var(--c-border);
  background: transparent;
  border-radius: var(--r-pill);
  font-size: 11px;
  font-family: var(--font);
  color: var(--c-text-2);
  padding: 2px 10px;
}
.m.on {
  background: var(--c-primary-bg);
  border-color: var(--c-primary);
  color: var(--c-primary);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.fchip {
  border: 1px solid var(--c-border);
  background: transparent;
  border-radius: var(--r-pill);
  font-size: 11.5px;
  font-family: var(--font);
  color: var(--c-text-2);
  padding: 2px 9px;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fchip.on {
  background: var(--c-primary-bg);
  border-color: var(--c-primary);
  color: var(--c-primary);
}
.fchip.user {
  border-style: dashed;
}
.fc {
  margin-left: 4px;
  font-size: 10px;
  color: var(--c-text-3);
}
.dates {
  display: flex;
  align-items: center;
  gap: 6px;
}
.dates input {
  flex: 1;
  min-width: 0;
  height: 26px;
  border-radius: 7px;
  border: 1px solid var(--c-border);
  background: var(--c-field);
  font-size: 11px;
  font-family: var(--font);
  color: var(--c-text);
}
.sep {
  font-size: 11px;
  color: var(--c-text-3);
}

.results {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.totalbar {
  margin: 0;
  font-size: 11.5px;
  color: var(--c-text-3);
  text-align: right;
}
.err {
  margin: 0;
  font-size: 12px;
  color: var(--c-danger);
}
</style>
