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
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import AppIcon from './AppIcon.vue'
import CandidateList from './CandidateList.vue'
import { formatSize, iconOf, kindLabel, relativeTime } from '../core/format'
import { imageSrc } from '../core/preview'
import { livePath } from '../core/types'
import { shortPath } from '../core/text'
import type { BatchItem, DecideAck, FilePreview, TagRef } from '../core/types'

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
  /** BUG-007：删除（移入隔离区，可撤销） */
  (e: 'trash'): void
  (e: 'open'): void
  (e: 'reveal'): void
  /** 归档时改的名字；空串 = 恢复原名 */
  (e: 'rename', name: string): void
  /** 人加一个标签（后端落 kind='user'） */
  (e: 'addTag', name: string): void
  /** 删一个标签（type/time 会被后端 TAG_LOCKED 拒掉） */
  (e: 'removeTag', tagId: number): void
  /** Ctrl+点标签 → 去检索（T3.6）；普通点击不跳，避免整理台误触 */
  (e: 'tagClick', tagId: number): void
}>()

const f = computed(() => props.item.file)
const confirmed = computed(() => f.value.status === 'confirmed')
const handledElsewhere = computed(
  () => f.value.status === 'queued' || f.value.status === 'parked',
)
const noDirs = computed(() => props.dirCount === 0)
const primaryIsNewDir = computed(() => noDirs.value || props.item.candidates.length === 0)

/** 深度解读的一句话说明：有就显示，没有就不占位（「未解读」不写假占位） */
const summary = computed(() => {
  const v = (f.value.summary ?? '').trim()
  return v.length > 0 ? v : null
})
/** 标签层级：读过正文（deep）实心，只打过基础标签空心 */
const tagLevel = computed(() => (f.value.tagLevel === 'deep' ? 'deep' : 'basic'))

const targetPath = computed(() => {
  if (!confirmed.value) return null
  return props.ack?.targetRelPath ?? null
})

/** 摘要默认只显示 4 行，够判断「这是哪一单」，又不会把候选和按钮顶出屏幕 */
const expanded = ref(false)
/** 实测 .pv-text 是否被 CSS 裁掉：按钮出现与否只认这个，不认后端 truncated */
const textOverflowed = ref(false)
const textEl = ref<HTMLElement | null>(null)

function measureOverflow() {
  // 展开态的高度已不是「4 行裁切」语义，不能用它覆盖已测得的溢出标记，否则「收起」会消失
  if (expanded.value) return
  const el = textEl.value
  textOverflowed.value = !!el && el.scrollHeight > el.clientHeight
}

let resizeObserver: ResizeObserver | null = null
/** happy-dom 没有 ResizeObserver：只在真实环境挂，窗口 resize 兜底 */
function syncObserver() {
  resizeObserver?.disconnect()
  resizeObserver = null
  if (typeof ResizeObserver !== 'undefined' && textEl.value) {
    resizeObserver = new ResizeObserver(() => measureOverflow())
    resizeObserver.observe(textEl.value)
  }
}
/** DOM 更新后再量一次并同步监听目标 */
function remeasure() {
  measureOverflow()
  syncObserver()
}
const onWindowResize = () => measureOverflow()

onMounted(() => {
  void nextTick(remeasure)
  window.addEventListener('resize', onWindowResize)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', onWindowResize)
  resizeObserver?.disconnect()
  resizeObserver = null
})

/** 换文件 / 换了正文：收起并重新测量，不残留上一份的展开态 */
watch(
  () => [props.item.file.id, props.preview?.kind, props.preview?.text] as const,
  () => {
    expanded.value = false
    textOverflowed.value = false
    void nextTick(remeasure)
  },
)

const thumb = computed(() =>
  props.preview?.kind === 'image' ? imageSrc(livePath(f.value)) : '',
)

/** 归档前改名（2026-11-20 反馈：想顺便把名字改得更明确） */
const renaming = ref(false)
const nameDraft = ref('')
const renameEl = ref<HTMLInputElement | null>(null)
/** 库里记的目标名；没改过是 null */
const targetName = computed(() => f.value.targetName || null)
const canRename = computed(() => !confirmed.value && !handledElsewhere.value)

/* ---- 标签：看得见 + 改得动（P3 T1.2 / T1.3） ---- */
const tags = computed<TagRef[]>(() => f.value.tags ?? [])
/** 超过 6 个折叠成 +N（title 里有全量） */
const MAX_TAGS = 6
const visibleTags = computed(() => tags.value.slice(0, MAX_TAGS))
const extraTags = computed(() => tags.value.length - visibleTags.value.length)
const extraTitle = computed(() =>
  tags.value
    .slice(MAX_TAGS)
    .map((t) => t.name)
    .join('、'),
)
/** type/time 是基础标签，删了就没法按类型/时间找，后端也会拒 */
const canRemove = (t: TagRef) => t.kind !== 'type' && t.kind !== 'time'

/** 标签行里的内联输入：+ 号点开，回车提交、Esc 收起 */
const addingTag = ref(false)
const tagDraft = ref('')
const tagEl = ref<HTMLInputElement | null>(null)

function startAddTag() {
  addingTag.value = true
  tagDraft.value = ''
  void nextTick(() => tagEl.value?.focus())
}

function commitAddTag() {
  const v = tagDraft.value.trim()
  addingTag.value = false
  tagDraft.value = ''
  if (v) emit('addTag', v)
}

/** 输入框里只放主干：扩展名由后端补回来，用户不用操心也不会被改坏 */
function stemOf(name: string): string {
  const ext = f.value.ext.toLowerCase()
  if (!ext) return name
  const suf = `.${ext}`
  return name.toLowerCase().endsWith(suf) ? name.slice(0, name.length - suf.length) : name
}

function startRename() {
  nameDraft.value = stemOf(targetName.value ?? f.value.name)
  renaming.value = true
  void nextTick(() => {
    renameEl.value?.focus()
    renameEl.value?.select()
  })
}

function commitRename() {
  if (!renaming.value) return
  const v = nameDraft.value.trim()
  renaming.value = false
  // 没动过就不必落库（避免每次点开再点掉都发一次 IPC）
  if (v === stemOf(targetName.value ?? f.value.name)) return
  emit('rename', v)
}

function cancelRename() {
  renaming.value = false
}
</script>

<template>
  <article class="card file-entry" :class="{ done: confirmed, aside: handledElsewhere }">
    <header class="hd">
      <div class="ico"><AppIcon :name="iconOf(f.kind)" :size="18" /></div>
      <div class="nm">
        <div class="nameline">
          <h2 v-if="!renaming" class="name" :title="f.name">{{ f.name }}</h2>
          <template v-else>
            <input
              ref="renameEl"
              v-model="nameDraft"
              class="rename-input"
              type="text"
              spellcheck="false"
              title="改完回车保存，Esc 取消"
              @keydown.enter.stop.prevent="commitRename"
              @keydown.esc.stop.prevent="cancelRename"
              @blur="commitRename"
            />
            <span v-if="f.ext" class="rename-ext">.{{ f.ext }}</span>
          </template>
          <button
            v-if="!renaming && canRename"
            class="ren"
            title="归档时换个名字（源文件不动）"
            @click="startRename"
          >
            <AppIcon name="edit" :size="11" />改名
          </button>
        </div>
        <p class="meta">
          <span>{{ formatSize(f.size) }}</span>
          <span class="dot">·</span>
          <span>{{ kindLabel(f.kind) }}</span>
          <span class="dot">·</span>
          <span>{{ relativeTime(f.mtime) }}</span>
          <span v-if="f.note" class="tag note">备注：{{ f.note }}</span>
          <!-- 空心 = 只按类型/时间打了基础标签；实心 = 读过正文。两者别混成一样 -->
          <span
            class="taglvl"
            :class="tagLevel"
            :title="tagLevel === 'deep' ? '已读过正文并给出结论' : '还没读过正文'"
          >
            {{ tagLevel === 'deep' ? '已解读' : '初步' }}
          </span>
        </p>
      </div>
      <div class="state">
        <span class="counter">{{ position.index + 1 }} / {{ position.total }}</span>
        <span v-if="deferredBadge" class="badge defer">已暂缓 {{ f.deferCount }} 次</span>
        <!-- 已经确认过就别再喊「待你指定」了，那是自相矛盾 -->
        <span v-if="item.needsYou && !noDirs && !confirmed" class="badge you">待你指定</span>
        <span v-if="confirmed" class="badge ok"><AppIcon name="check" :size="11" /> 已确认</span>
      </div>
    </header>

    <!-- 路径与打开动作单独一行：这是「这份文件到底在哪」的答案 -->
    <div class="pathrow">
      <span class="plabel">位置</span>
      <!-- 中间省略：左边留盘符、右边留「哪一份」，title 里是全文（T5.2） -->
      <span class="mono pval" :title="f.absPath">{{ shortPath(f.absPath) }}</span>
      <button class="act" :disabled="busy" title="用系统默认程序打开（O）" @click="emit('open')">
        <span class="kbd">O</span>打开
      </button>
      <button class="act" :disabled="busy" title="在资源管理器中显示（L）" @click="emit('reveal')">
        <span class="kbd">L</span>定位
      </button>
    </div>

    <!-- 改过名就说清楚：落盘用的是这个名字，源文件不动 -->
    <p v-if="targetName && !renaming" class="newname">
      <AppIcon name="edit" :size="11" />
      归档时存为 <b>{{ targetName }}</b>
      <button v-if="canRename" class="link" @click="startRename">再改</button>
      <button v-if="canRename" class="link" @click="emit('rename', '')">用原名</button>
    </p>

    <!-- 标签行：type/time 灰底、topic 蓝底实心、user 虚线框；0 标签整个不渲染（不留空行） -->
    <div v-if="tags.length > 0" class="tagsrow">
      <span class="tlabel">标签</span>
      <span
        v-for="t in visibleTags"
        :key="t.id"
        class="chip"
        :class="t.kind"
        :title="(t.kind === 'user' ? '你自己加的（Ctrl+点去检索）' : t.kind === 'topic' ? 'AI 深读给的（Ctrl+点去检索）' : t.kind === 'type' || t.kind === 'time' ? '基础标签（不可删）' : '扫描时按词匹配的（Ctrl+点去检索）')"
        @click="(e) => e.ctrlKey && emit('tagClick', t.id)"
      >
        {{ t.name }}
        <button
          v-if="canRemove(t)"
          class="x"
          title="删掉这个标签"
          @click="emit('removeTag', t.id)"
        >×</button>
      </span>
      <span
        v-if="extraTags > 0"
        class="chip more"
        :title="extraTitle"
      >+{{ extraTags }}</span>
      <button v-if="!addingTag" class="add" title="加一个自己的标签" @click="startAddTag">+</button>
      <input
        v-else
        ref="tagEl"
        v-model="tagDraft"
        class="tag-input"
        type="text"
        placeholder="标签名，回车确认"
        spellcheck="false"
        @keydown.enter.stop.prevent="commitAddTag"
        @keydown.esc.stop.prevent="addingTag = false"
        @blur="commitAddTag"
      />
    </div>

    <!-- 深度解读的一句话结论：没有就不占位 -->
    <p v-if="summary" class="summary" :title="summary">
      <AppIcon name="sparkle" :size="12" />
      {{ summary }}
    </p>

    <!-- 正文摘要 / 图片缩略图 / 读不出的原因 -->
    <section v-if="preview" class="preview">
      <template v-if="preview.kind === 'image'">
        <button v-if="thumb" class="pv-thumb" title="点击用默认程序打开" @click="emit('open')">
          <img :src="thumb" :alt="f.name" loading="lazy" />
        </button>
        <p v-else class="pv-none">图片预览只在应用内可用，直接打开看</p>
      </template>
      <p v-else-if="preview.kind === 'text'" ref="textEl" class="pv-text" :class="{ full: expanded }">
        {{ preview.text }}
      </p>
      <p v-else class="pv-none">{{ preview.reason }}</p>

      <button
        v-if="preview.kind === 'text' && textOverflowed"
        class="pv-toggle"
        @click="expanded = !expanded"
      >
        {{ expanded ? '收起' : '展开全文' }}
        <AppIcon :name="expanded ? 'chevron-down' : 'chevron-right'" :size="11" />
      </button>
    </section>

    <p v-if="confirmed" class="result">
      <AppIcon name="check" :size="13" />
      <!-- BUG-005：长路径单独成块，两行后省略；title 永远是完整值 -->
      <span class="dest" :title="targetPath ?? '所选目录'">
        已归到 <b>{{ targetPath ?? '所选目录' }}</b>
      </span>
      <span v-if="f.targetName" class="as">存为 {{ f.targetName }}</span>
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
      <!-- BUG-007：待确认 / 已确认都能删。文案只说「移入隔离区，可撤销」，
           由 Workspace 的二次确认真正执行 -->
      <button
        v-if="!handledElsewhere"
        class="btn trash-btn"
        :disabled="busy"
        title="移入隔离区，可撤销（Delete）；原文件可取回"
        @click="emit('trash')"
      >
        <span class="kbd">Del</span> 删除（移入隔离区）
      </button>
      <span v-if="confirmed || handledElsewhere" class="hint">按 ↓ 看下一份，Ctrl+Z 撤销</span>
    </footer>
  </article>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  /* 底色 / 边框 / 圆角 / 内边距 / 间距 / 字号统一由全局 .file-entry + tokens 提供（BUG-008） */
  box-shadow: var(--sh-card);
  /* BUG-005：卡片不是滚动容器，不给它设最小高度为 0（项目踩坑：子项被压扁溢出） */
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

/* BUG-005：图标 / 标题 / 徽章分列。标题列 minmax(0,1fr) 保证长名省略而不被徽章顶破，
   徽章列封顶 45% 迫使多徽章换行；显式 column-gap 保住标题与徽章的最小间距 */
.hd {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr) minmax(0, 45%);
  align-items: start;
  column-gap: 12px;
  row-gap: 6px;
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
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---- 改名 ---- */
.nameline {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.nameline .name {
  min-width: 0;
}
.ren {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  border: 0;
  background: transparent;
  border-radius: 6px;
  padding: 2px 5px;
  font-size: 11px;
  font-family: var(--font);
  color: var(--c-text-3);
  /* 常驻但压暗：只在 hover 才出现的话，没人知道这儿能改名 */
  opacity: 0.6;
  transition: opacity var(--dur-fast) ease, color var(--dur-fast) ease;
}
/* 指到卡片上就提亮 */
.card:hover .ren,
.ren:focus-visible {
  opacity: 1;
}
.ren:hover {
  color: var(--c-primary);
  background: var(--c-primary-bg);
}
.rename-input {
  flex: 1 1 auto;
  min-width: 0;
  height: 30px;
  padding: 0 8px;
  border-radius: 7px;
  border: 1px solid var(--c-primary);
  background: var(--c-surface);
  box-shadow: 0 0 0 3px var(--c-primary-glow);
  font-size: 14px;
  font-weight: 600;
  font-family: var(--font);
  color: var(--c-text);
  outline: none;
}
.rename-ext {
  flex: 0 0 auto;
  font-size: 13px;
  color: var(--c-text-2);
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
}

.newname {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: 6px 10px;
  border-radius: var(--r-item);
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-size: 12px;
  min-width: 0;
}
.newname b {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.link {
  flex: 0 0 auto;
  border: 0;
  background: transparent;
  padding: 0;
  font-size: 11px;
  font-family: var(--font);
  color: var(--c-primary);
  text-decoration: underline;
  text-underline-offset: 2px;
}
.link:first-of-type {
  margin-left: auto;
}

.meta {
  margin: 3px 0 0;
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
  font-size: var(--fs-chip);
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
  row-gap: 4px;
  flex-wrap: wrap;
  justify-content: flex-end;
  align-content: start;
  min-width: 0;
  max-width: 45%;
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

.taglvl {
  flex: 0 0 auto;
  font-size: 10px;
  line-height: 1.6;
  padding: 0 6px;
  border-radius: var(--r-pill);
  border: 1px solid var(--c-border-strong);
  color: var(--c-text-2);
  background: transparent;
}
/* 实心：读过正文，结论可信 */
.taglvl.deep {
  border-color: transparent;
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
  font-weight: 600;
}

.summary {
  margin: 0;
  display: flex;
  align-items: flex-start;
  gap: 5px;
  color: var(--c-text-2);
  min-width: 0;
}

/* ---- 标签行（P3 T1.2） ---- */
.tagsrow {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 5px;
  min-width: 0;
}
.tlabel {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--c-text-3);
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 8px;
  border-radius: var(--r-pill);
  background: var(--c-field);
  color: var(--c-text-2);
  border: 1px solid transparent;
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 基础标签灰底（默认样式就是），模型话题蓝底实心，用户标签虚线框 */
.chip.topic {
  background: var(--c-indigo-bg);
  color: var(--c-indigo);
  font-weight: 500;
}
.chip.user {
  background: transparent;
  border: 1px dashed var(--c-border-strong);
  color: var(--c-text);
}
.chip.more {
  background: transparent;
  color: var(--c-text-3);
}
.chip .x {
  border: 0;
  background: transparent;
  padding: 0 1px;
  font-size: 12px;
  line-height: 1;
  color: inherit;
  opacity: 0.45;
  cursor: pointer;
}
.chip .x:hover {
  opacity: 1;
  color: var(--c-danger);
}
.add {
  border: 1px dashed var(--c-border-strong);
  background: transparent;
  border-radius: var(--r-pill);
  padding: 0 8px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--c-text-3);
}
.add:hover {
  color: var(--c-primary);
  border-color: var(--c-primary);
}
.tag-input {
  height: 22px;
  width: 130px;
  padding: 0 8px;
  border-radius: var(--r-pill);
  border: 1px solid var(--c-primary);
  background: var(--c-surface);
  font-size: 11px;
  font-family: var(--font);
  color: var(--c-text);
  outline: none;
  box-shadow: 0 0 0 2px var(--c-primary-glow);
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
  /* -webkit-box + overflow:auto 在 WebView2 里滚不动，展开态改成普通块级 */
  display: block;
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
  row-gap: 2px;
  flex-wrap: wrap;
  min-width: 0;
  margin: 0;
  padding: 9px 12px;
  border-radius: var(--r-banner);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  font-size: 13px;
  animation: result-in var(--dur) var(--ease-out) both;
}
/* BUG-005：归档路径最多两行后省略，绝不把绿色结果行撑成一大块 */
.result .dest {
  min-width: 0;
  flex: 1 1 60%;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  overflow-wrap: anywhere;
  line-height: var(--lh-file);
}
.result .dest b {
  font-weight: 600;
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
.result .as {
  margin-left: auto;
  font-size: 11px;
  color: var(--c-text-2);
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* BUG-005：.body 不是滚动容器，删掉 min-height:0，避免候选区被压扁溢出 */

.ft {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  border-top: 0.5px solid var(--c-border);
  padding-top: 12px;
  margin-top: auto;
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

/* BUG-007：删除入口压暗但常驻 —— 用危险色提示后果，又不像主按钮那样诱导误点 */
.trash-btn {
  color: var(--c-danger);
}
.trash-btn:hover {
  background: var(--c-danger-bg);
}
</style>
