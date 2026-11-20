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

import { computed, onMounted, ref } from 'vue'
import Card from '../../components/Card.vue'
import AppIcon from '../../components/AppIcon.vue'
import { useAppStore } from '../../stores/app'
import * as ipc from '../../core/ipc'
import { formatSize, kindLabel } from '../../core/format'
import { humanize, toCmdError } from '../../core/cmdError'
import type { ParkedItem } from '../../core/types'

const app = useAppStore()

const items = ref<ParkedItem[]>([])
const loading = ref(false)
const busy = ref<number | null>(null)
const err = ref<string | null>(null)

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
      <li v-for="it in items" :key="it.file.id" class="row" :class="{ busy: busy === it.file.id }">
        <div class="head">
          <AppIcon name="doc" :size="14" />
          <span class="name ellipsis" :title="it.file.absPath">{{ it.file.name }}</span>
          <span class="meta">{{ kindLabel(it.file.kind) }} · {{ formatSize(it.file.size) }}</span>
        </div>

        <!-- 没读过就直说「未解读」，不做假占位（P2 的硬要求） -->
        <p class="sum" :class="{ none: !it.file.summary }">
          {{ it.file.summary || '未解读' }}
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
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--c-field);
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
  font-size: 13px;
  font-weight: 500;
  color: var(--c-text);
  min-width: 0;
  flex: 1;
}

.meta {
  font-size: 11px;
  color: var(--c-text-3);
  flex: 0 0 auto;
}

.sum {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--c-text);
}
.sum.none {
  color: var(--c-text-3);
}

.note {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
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
  font-size: 11px;
  color: var(--c-text-3);
  line-height: 1.5;
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
