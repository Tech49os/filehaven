<script setup lang="ts">
/**
 * 整理台的主卡片（T6.10）
 *
 * 一份文件一屏一个，所有可点的东西都带键盘提示 ——
 * 用户全程不碰鼠标也能走完一批（验收 3）。
 *
 * 2026-11-20 加了两件事（实测反馈「只能看到文件名，路径和内容看不到，也无法快速打开」）：
 * 1. 完整绝对路径单独一行 —— 之前它挤在「大小·类型·时间·路径」那行末尾，被省略成只剩文件名；
 * 2. 正文摘要 / 图片缩略图 + 「打开」「定位」。判定一份 `扫描件001.pdf` 靠文件名是不行的。
 */
import { computed, ref } from 'vue'
import AppIcon from './AppIcon.vue'
import CandidateList from './CandidateList.vue'
import { formatSize, iconOf, kindLabel, relativeTime } from '../core/format'
import { imageSrc } from '../core/preview'
import type { BatchItem, DecideAck, FilePreview } from '../core/types'

const props = withDefaults(
  defineProps<{
    item: BatchItem
    dirCount: number
    position: { index: number; total: number }
    deferredBadge?: boolean
    ack?: DecideAck | null
    busy?: boolean
    /** 正文/图片预览；null = 还没取回来（此时不占位，避免闪一个空框） */
    preview?: FilePreview | null
  }>(),
  { deferredBadge: false, ack: null, busy: false, preview: null },
)

const emit = defineEmits<{
  (e: 'accept'): void
  (e: 'pick', n: 1 | 2 | 3): void
  (e: 'newDir'): void
  (e: 'defer'): void
  (e: 'park'): void
  (e: 'open'): void
  (e: 'reveal'): void
}>()

const f = computed(() => props.item.file)
const confirmed = computed(() => f.value.status === 'confirmed')
const handledElsewhere = computed(
  () => f.value.status === 'queued' || f.value.status === 'parked',
)
const noDirs = computed(() => props.dirCount === 0)
const primaryIsNewDir = computed(() => noDirs.value || props.item.candidates.length === 0)

const targetPath = computed(() => {
  if (!confirmed.value) return null
  return props.ack?.targetRelPath ?? null
})

/** 摘要默认只显示 4 行，够判断「这是哪一单」，又不会把候选和按钮顶出屏幕 */
const expanded = ref(false)
const thumb = computed(() =>
  props.preview?.kind === 'image' ? imageSrc(f.value.absPath) : '',
)
</script>

<template>
  <article class="card" :class="{ done: confirmed, aside: handledElsewhere }">
    <header class="hd">
      <div class="ico"><AppIcon :name="iconOf(f.kind)" :size="18" /></div>
      <div class="nm">
        <h2 class="name" :title="f.name">{{ f.name }}</h2>
        <p class="meta">
          <span>{{ formatSize(f.size) }}</span>
          <span class="dot">·</span>
          <span>{{ kindLabel(f.kind) }}</span>
          <span class="dot">·</span>
          <span>{{ relativeTime(f.mtime) }}</span>
          <span v-if="f.note" class="tag note">备注：{{ f.note }}</span>
        </p>
      </div>
      <div class="state">
        <span class="counter">{{ position.index + 1 }} / {{ position.total }}</span>
        <span v-if="deferredBadge" class="badge defer">已暂缓 {{ f.deferCount }} 次</span>
        <span v-if="item.needsYou && !noDirs" class="badge you">待你指定</span>
        <span v-if="confirmed" class="badge ok"><AppIcon name="check" :size="11" /> 已确认</span>
      </div>
    </header>

    <!-- 路径与打开动作单独一行：这是「这份文件到底在哪」的答案 -->
    <div class="pathrow">
      <span class="plabel">位置</span>
      <span class="mono pval" :title="f.absPath">{{ f.absPath }}</span>
      <button class="act" :disabled="busy" title="用系统默认程序打开（O）" @click="emit('open')">
        <span class="kbd">O</span>打开
      </button>
      <button class="act" :disabled="busy" title="在资源管理器中显示（L）" @click="emit('reveal')">
        <span class="kbd">L</span>定位
      </button>
    </div>

    <!-- 正文摘要 / 图片缩略图 / 读不出的原因 -->
    <section v-if="preview" class="preview">
      <template v-if="preview.kind === 'image'">
        <button v-if="thumb" class="pv-thumb" title="点击用默认程序打开" @click="emit('open')">
          <img :src="thumb" :alt="f.name" loading="lazy" />
        </button>
        <p v-else class="pv-none">图片预览只在应用内可用，直接打开看</p>
      </template>
      <p v-else-if="preview.kind === 'text'" class="pv-text" :class="{ full: expanded }">
        {{ preview.text }}
      </p>
      <p v-else class="pv-none">{{ preview.reason }}</p>

      <button
        v-if="preview.kind === 'text' && preview.truncated"
        class="pv-toggle"
        @click="expanded = !expanded"
      >
        {{ expanded ? '收起' : '展开全文' }}
        <AppIcon :name="expanded ? 'chevron-down' : 'chevron-right'" :size="11" />
      </button>
    </section>

    <p v-if="confirmed" class="result">
      <AppIcon name="check" :size="13" />
      已归到 <b>{{ targetPath ?? '所选目录' }}</b>
    </p>
    <p v-else-if="handledElsewhere" class="result dim">
      {{ f.status === 'parked' ? '已放进待定区，回头再定' : '已暂缓，回到池子里下批再说' }}
    </p>

    <div class="body">
      <CandidateList
        v-if="!confirmed && !handledElsewhere"
        :candidates="item.candidates"
        :needs-you="item.needsYou"
        :dir-count="dirCount"
        :disabled="busy"
        @pick="emit('pick', $event)"
      />
    </div>

    <footer class="ft">
      <button
        v-if="!confirmed && !handledElsewhere"
        class="btn primary"
        :disabled="busy"
        @click="primaryIsNewDir ? emit('newDir') : emit('accept')"
      >
        <span class="kbd">回车</span>
        {{ primaryIsNewDir ? '新建目录' : `归到 ${item.candidates[0]?.relPath ?? ''}` }}
      </button>
      <button
        v-if="!confirmed && !handledElsewhere && !primaryIsNewDir"
        class="btn"
        :disabled="busy"
        @click="emit('newDir')"
      >
        <span class="kbd">N</span> 新建目录
      </button>
      <button
        v-if="!confirmed && !handledElsewhere"
        class="btn"
        :disabled="busy"
        @click="emit('defer')"
      >
        <span class="kbd">S</span> 暂缓
      </button>
      <button
        v-if="!confirmed && !handledElsewhere"
        class="btn"
        :disabled="busy"
        @click="emit('park')"
      >
        <span class="kbd">P</span> 先放着
      </button>
      <span v-if="confirmed || handledElsewhere" class="hint">按 ↓ 看下一份，Ctrl+Z 撤销</span>
    </footer>
  </article>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  padding: 18px 20px;
  min-height: 0;
  /* 换一份文件 = 组件重挂（key 变了），这一下淡入就是「换张卡」的反馈 */
  animation: card-in var(--dur) var(--ease-out) both;
  transition: border-color var(--dur) ease, box-shadow var(--dur) ease;
}
@keyframes card-in {
  from {
    opacity: 0;
    transform: translateY(5px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.card.done {
  border-color: rgba(40, 200, 64, 0.35);
  box-shadow: 0 0 0 3px var(--c-success-bg);
}
.card.aside {
  border-color: rgba(255, 149, 0, 0.3);
}

.hd {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.ico {
  width: 34px;
  height: 34px;
  flex: 0 0 34px;
  border-radius: 9px;
  background: var(--c-field);
  display: grid;
  place-items: center;
  color: var(--c-text-2);
}

.nm {
  min-width: 0;
  flex: 1;
}

.name {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  margin: 3px 0 0;
  font-size: 12px;
  color: var(--c-text-2);
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
}
.meta > span {
  flex: 0 0 auto;
}
.meta .dot {
  color: var(--c-text-3);
}

.tag {
  font-size: 11px;
  padding: 1px 7px;
  border-radius: var(--r-pill);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tag.note {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}

.state {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  justify-content: flex-end;
  flex: 0 0 auto;
}

.counter {
  font-size: 11px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  padding: 2px 7px;
  border-radius: var(--r-badge);
  white-space: nowrap;
}
.badge.defer {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}
.badge.you {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
}
.badge.ok {
  background: var(--c-success-bg);
  color: var(--c-success-deep);
}

/* ---- 路径行 ---- */
.pathrow {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: var(--r-item);
  background: var(--c-field);
  min-width: 0;
}
.plabel {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--c-text-3);
}
.pval {
  flex: 1 1 auto;
  min-width: 0;
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 11px;
  color: var(--c-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.act {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  border: 0;
  border-radius: 7px;
  padding: 3px 8px;
  font-size: 11px;
  font-family: var(--font);
  background: var(--c-surface);
  color: var(--c-text-2);
  box-shadow: var(--sh-control);
}
.act:hover {
  color: var(--c-primary);
}
.act:disabled {
  opacity: 0.5;
}

/* ---- 预览 ---- */
.preview {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}
/* 摘要/说明带一层底色：跟上面的路径行区分开，一眼看得出「这是文件里的字」 */
.pv-text,
.pv-none {
  margin: 0;
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--r-item);
  background: var(--c-field);
  font-size: 12.5px;
  line-height: 1.75;
  color: var(--c-text);
  white-space: pre-wrap;
  word-break: break-word;
}
.pv-text {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
  overflow: hidden;
}
.pv-text.full {
  -webkit-line-clamp: unset;
  overflow: auto;
  max-height: 220px;
}
.pv-none {
  color: var(--c-text-3);
}
.pv-thumb {
  align-self: flex-start;
  border: 0;
  padding: 0;
  background: var(--c-field);
  border-radius: var(--r-item);
  overflow: hidden;
  max-width: 100%;
  line-height: 0;
}
.pv-thumb img {
  display: block;
  max-width: 100%;
  max-height: 190px;
  object-fit: contain;
}
.pv-toggle {
  align-self: flex-start;
  border: 0;
  background: transparent;
  color: var(--c-primary);
  font-size: 11px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.result {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: 9px 12px;
  border-radius: var(--r-banner);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  font-size: 13px;
  animation: result-in var(--dur) var(--ease-out) both;
}
@keyframes result-in {
  from {
    opacity: 0;
    transform: translateY(3px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.result.dim {
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
}

.body {
  min-height: 0;
}

.ft {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  border-top: 0.5px solid var(--c-border);
  padding-top: 12px;
  margin-top: auto;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 0;
  border-radius: var(--r-btn);
  padding: 7px 12px;
  font-size: 12px;
  background: var(--c-field);
  color: var(--c-text);
  font-family: var(--font);
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn:disabled {
  opacity: 0.55;
}

.kbd {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 5px;
  background: rgba(255, 255, 255, 0.22);
  border: 0.5px solid rgba(255, 255, 255, 0.3);
}
.btn:not(.primary) .kbd,
.act .kbd {
  background: var(--c-surface);
  border-color: var(--c-border-strong);
  color: var(--c-text-3);
}

.hint {
  margin-left: auto;
  font-size: 11px;
  color: var(--c-text-3);
}

/* 窗口不高时，卡片自己先松一档内边距，把空间让给内容 */
@media (max-height: 800px) {
  .card {
    padding: 14px 16px;
    gap: 8px;
  }
}
</style>
