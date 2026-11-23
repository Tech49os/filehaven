<script setup lang="ts">
/**
 * 待定区（P2 T4.3）
 *
 * 在整理台按 `P`「先放着」的文件进这里。P1 里这里是死胡同 ——
 * 文件进了 `parked` 就再也回不来，这不是「少个功能」，是承诺过的动作没兑现。
 *
 * 三条出路，一条都不许缺：
 *   - 直接指定：点主推 / 备选目录 → `parked → confirmed`，进待迁移
 *   - 捞回：`parked → pending_confirm`，下一批整理台会再问一次
 *   - 移出：`parked → skipped`，不当成丢失，也不再出现
 *
 * 候选是**每次打开现算的**：这些文件可能压了几周，这期间目录长了、
 * 别的文件归档过了，开批时算的候选早就不作数。
 */

import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import Card from '../../components/Card.vue'
import AppIcon from '../../components/AppIcon.vue'
import { useAppStore } from '../../stores/app'
import * as ipc from '../../core/ipc'
import { onDeepTagProgress } from '../../core/events'
import { formatSize, kindLabel } from '../../core/format'
import { humanize, toCmdError } from '../../core/cmdError'
import type { DeepTagStats, ParkedItem } from '../../core/types'

const app = useAppStore()
const router = useRouter()

const items = ref<ParkedItem[]>([])
const loading = ref(false)
const busy = ref<number | null>(null)
const err = ref<string | null>(null)

// —— 「让 AI 看看这是什么」（P3 T4.5） ——
const deep = ref<DeepTagStats | null>(null)
/** 点了还没读到结论的文件（按钮中间态「正在读…」） */
const explaining = ref<number[]>([])
let lastDone = 0
let stopDeep: (() => void) | null = null

const deepOff = computed(() => deep.value !== null && !deep.value.enabled)
const reading = (id: number) => explaining.value.includes(id)

async function askExplain(it: ParkedItem) {
  if (reading(it.file.id)) return
  try {
    // 不同步等待：立刻回 queued，结论走 deep-tag-progress 事件
    await ipc.explainFile(it.file.id)
    explaining.value = [...explaining.value, it.file.id]
    err.value = null
  } catch (e) {
    err.value = humanize(toCmdError(e))
  }
}

/** 静默刷新：不闪 loading，行内 summary 原地更新 */
async function refreshSilently() {
  try {
    items.value = await ipc.listParked()
  } catch {
    /* 保持原样，下一轮事件再试 */
  }
}

const empty = computed(() => !loading.value && items.value.length === 0)

async function load() {
  loading.value = true
  try {
    items.value = await ipc.listParked()
    err.value = null
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  void load()
  try {
    const s = await ipc.deepTagStatus()
    deep.value = s ?? null
    lastDone = s?.done ?? 0
  } catch {
    /* 拿不到状态就当开着，点了真关着会收到后端的明确报错 */
  }
  stopDeep = null
  void onDeepTagProgress((snap) => {
    deep.value = snap
    if (explaining.value.length === 0) {
      lastDone = snap.done
      return
    }
    // 我们在等结论：done 前进或队列空闲 → 该份读完了，收敛等待集并原地刷新
    if (snap.done > lastDone || !snap.running) {
      lastDone = snap.done
      explaining.value = explaining.value.filter((id) => snap.currentId === id)
      void refreshSilently()
    }
  }).then((off) => {
    stopDeep = off
  })
})

onUnmounted(() => stopDeep?.())

/** 三个动作都是写操作：做完刷计数，行从列表里消失（而不是整页闪一下重载） */
async function act(fileId: number, fn: () => Promise<void>) {
  busy.value = fileId
  try {
    await fn()
    items.value = items.value.filter((it) => it.file.id !== fileId)
    await app.refreshBoot()
    err.value = null
  } catch (e) {
    err.value = humanize(toCmdError(e))
  } finally {
    busy.value = null
  }
}

const assign = (it: ParkedItem, dirId: number) =>
  act(it.file.id, () => ipc.assignParked(it.file.id, dirId, 'user_picked'))
const unpark = (it: ParkedItem) => act(it.file.id, () => ipc.unpark(it.file.id))
const drop = (it: ParkedItem) => act(it.file.id, () => ipc.dropFile(it.file.id))

onMounted(load)
</script>

<template>
  <Card title="待定区">
    <template #action>
      <button class="btn ghost" :disabled="loading" @click="load">刷新</button>
    </template>

    <p v-if="err" class="hint bad">{{ err }}</p>

    <div v-if="empty" class="ph">
      <AppIcon name="clock" :size="26" />
      <p class="t">这里现在是空的</p>
      <p class="d">
        整理台里按 <kbd>P</kbd>「先放着」的文件会进到这里，等你哪天想清楚了再来定。
        它们不会丢，也不会自己跑掉。
      </p>
    </div>

    <p v-else-if="loading && items.length === 0" class="hint">正在算候选…</p>

    <ul v-else class="list">
      <li
        v-for="it in items"
        :key="it.file.id"
        class="row file-entry"
        :class="{ busy: busy === it.file.id }"
      >
        <div class="head">
          <AppIcon name="doc" :size="14" />
          <span class="name ellipsis" :title="it.file.absPath">{{ it.file.name }}</span>
          <span class="meta">{{ kindLabel(it.file.kind) }} · {{ formatSize(it.file.size) }}</span>
        </div>

        <!-- 没读过就直说「未解读」，不做假占位（P2 的硬要求） -->
        <p class="sum" :class="{ none: !it.file.summary }">
          {{ it.file.summary || '未解读' }}
        </p>

        <!-- 标签一行（P3 T1.2）：超过 6 个折叠成 +N，title 给全量；0 标签不渲染 -->
        <p
          v-if="(it.file.tags ?? []).length > 0"
          class="tags"
          :title="(it.file.tags ?? []).map((t) => t.name).join('、')"
        >
          <span
            v-for="t in (it.file.tags ?? []).slice(0, 6)"
            :key="t.id"
            class="tchip"
            :class="t.kind"
          >{{ t.name }}</span>
          <span v-if="it.file.tags!.length > 6" class="tchip more">+{{ it.file.tags!.length - 6 }}</span>
        </p>

        <p v-if="it.file.note" class="note">备注：{{ it.file.note }}</p>

        <div class="acts">
          <template v-if="it.candidates.length > 0">
            <button
              v-for="(c, i) in it.candidates"
              :key="c.dirId"
              class="btn"
              :class="{ ghost: i > 0 }"
              :disabled="busy !== null"
              :title="c.reason"
              @click="assign(it, c.dirId)"
            >
              {{ i === 0 ? '主推 · ' : '' }}{{ c.relPath }}
            </button>
          </template>
          <span v-else class="you">这份没有足够像的目录，得你自己建一个</span>

          <span class="spacer" />
          <template v-if="deepOff">
            <span class="you">深度解读已关闭</span>
            <button class="btn ghost" @click="router.push('/settings')">去开启</button>
          </template>
          <button
            v-else-if="!it.file.summary"
            class="btn ghost"
            :disabled="busy !== null || reading(it.file.id)"
            @click="askExplain(it)"
          >
            {{ reading(it.file.id) ? '正在读…' : '让 AI 看看这是什么' }}
          </button>
          <button class="btn ghost" :disabled="busy !== null" @click="unpark(it)">捞回</button>
          <button class="btn ghost" :disabled="busy !== null" @click="drop(it)">移出</button>
        </div>

        <p v-if="it.candidates.length > 0" class="why">{{ it.candidates[0].reason }}</p>
      </li>
    </ul>
  </Card>
</template>

<style scoped>
.list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.row {
  /* 规范条目：底色 / 边框 / 圆角 / 内边距 / 间距由全局 .file-entry 统一提供（BUG-008） */
  display: flex;
  flex-direction: column;
}
.row.busy {
  opacity: 0.6;
}

.head {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  color: var(--c-text-2);
}

.name {
  font-weight: 600;
  color: var(--c-text);
  min-width: 0;
  flex: 1;
}

.meta {
  color: var(--c-text-3);
  flex: 0 0 auto;
}

.sum {
  margin: 0;
  color: var(--c-text);
}
.sum.none {
  color: var(--c-text-3);
}

.note {
  margin: 0;
  color: var(--c-text-2);
}

/* 标签一行（P3 T1.2）：样式与卡片一致 —— topic 蓝底、user 虚线、基础灰底 */
.tags {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  margin: 0;
  min-width: 0;
}
.tchip {
  padding: 0 7px;
  border-radius: var(--r-pill);
  background: var(--c-field);
  color: var(--c-text-2);
  border: 1px solid transparent;
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tchip.topic {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}
.tchip.user {
  background: transparent;
  border: 1px dashed var(--c-border-strong);
  color: var(--c-text);
}
.tchip.more {
  background: transparent;
  color: var(--c-text-3);
}

.acts {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  margin-top: 2px;
}

/* 主推按钮上的目录名可能很长，别把整行撑爆 */
.acts .btn {
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.spacer {
  flex: 1;
}

.you {
  font-size: 12px;
  color: var(--c-text-3);
}

.why {
  margin: 0;
  color: var(--c-text-3);
}

.ph {
  display: grid;
  place-items: center;
  gap: 6px;
  color: var(--c-text-3);
  padding: 36px 12px;
}
.t {
  margin: 6px 0 0;
  font-size: 14px;
  color: var(--c-text-2);
}
.d {
  margin: 0;
  max-width: 460px;
  text-align: center;
  font-size: 12px;
  line-height: 1.7;
}
kbd {
  font-family: var(--font);
  font-size: 11px;
  padding: 1px 5px;
  border-radius: 5px;
  border: 0.5px solid var(--c-border-strong);
  background: var(--c-field);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
