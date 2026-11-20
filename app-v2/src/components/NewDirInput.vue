<script setup lang="ts">
/**
 * 新建目录输入框（T6.11）
 *
 * 输入 `父/子`，300ms 防抖后调后端 `preview_dir_name` 拿清洗结果，
 * 清洗后为空就禁用确认并说明原因 —— 用户改的是「写什么」，
 * 系统告诉用户「会变成什么」，不偷偷改完不吭声。
 *
 * 2026-11-20 反馈：「在 N 输入框中要有联想能力，能够快速索引到」。
 * 原来只挂了一个 `<datalist>`：WebView2 里它长得像浏览器的自动填充，中文输入法下
 * 基本弹不出来，也没有「这是已有目录」的信号。现在换成自己画的下拉：
 * 敲几个字就按 relPath 过滤，↑↓ 选、回车直接归到这个目录（后端逐段查重，不会建重复目录）。
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import * as ipc from '../core/ipc'
import { toCmdError } from '../core/cmdError'
import { useDirsStore } from '../stores/dirs'
import type { DirNamePreview } from '../core/types'

const props = withDefaults(
  defineProps<{
    placeholder?: string
    /** 打开时把已有目录列进补全 */
    withSuggestions?: boolean
  }>(),
  { placeholder: '例如：安全检查/年度检查', withSuggestions: true },
)

const emit = defineEmits<{
  (e: 'confirm', input: string): void
  (e: 'cancel'): void
  (e: 'input-open', open: boolean): void
}>()

const dirs = useDirsStore()
const text = ref('')
const preview = ref<DirNamePreview | null>(null)
const checking = ref(false)
const err = ref<string | null>(null)
const inputEl = ref<HTMLInputElement | null>(null)

/** 下拉是否展开 + 高亮第几条（-1 = 没选） */
const listOpen = ref(false)
const active = ref(-1)

let timer: ReturnType<typeof setTimeout> | null = null
let seq = 0

/** 输入框是否打开 —— 供 App 层判断要不要抢键盘 */
const open = ref(true)

onMounted(() => {
  inputEl.value?.focus()
  emit('input-open', true)
  if (props.withSuggestions) {
    // 每次都拉一遍：刚在别处建过目录，缓存就是旧的。
    // 拉完不主动展开——空输入框上直接盖一层下拉太吵，聚焦或敲字时再出来
    void (async () => {
      await dirs.load()
    })()
  }
})

onUnmounted(() => {
  if (timer) clearTimeout(timer)
  emit('input-open', false)
})

const query = computed(() => text.value.trim())

/**
 * 候选目录：按输入过滤。
 *
 * 排序规则是「前缀命中优先，其次层级浅的优先」—— 敲「安全」时
 * 「安全检查」应该排在「2024/安全检查/复查」前面。
 */
const matches = computed(() => {
  if (!props.withSuggestions) return []
  const all = dirs.allPaths
  const q = query.value.toLowerCase()
  const hit = q
    ? all.filter((p) => p.toLowerCase().includes(q))
    : all.slice()
  hit.sort((a, b) => {
    const pa = q && a.toLowerCase().startsWith(q) ? 0 : 1
    const pb = q && b.toLowerCase().startsWith(q) ? 0 : 1
    if (pa !== pb) return pa - pb
    const da = a.split('/').length
    const db = b.split('/').length
    if (da !== db) return da - db
    return a.localeCompare(b, 'zh')
  })
  return hit.slice(0, 6)
})

/** 输入的就是已有目录？是的话回车＝归到这里，不是新建 */
const isExisting = computed(
  () => query.value.length > 0 && dirs.allPaths.includes(query.value),
)

watch(text, (v) => {
  err.value = null
  active.value = -1
  listOpen.value = props.withSuggestions && dirs.count > 0
  if (timer) clearTimeout(timer)
  const trimmed = v.trim()
  if (!trimmed) {
    preview.value = null
    checking.value = false
    return
  }
  // 本地先给个「正在检查」，避免 300ms 内按钮状态乱跳
  checking.value = true
  timer = setTimeout(async () => {
    const mine = ++seq
    try {
      const p = await ipc.previewDirName(trimmed)
      if (mine !== seq) return
      preview.value = p
    } catch (e) {
      if (mine !== seq) return
      err.value = toCmdError(e).message
      preview.value = null
    } finally {
      if (mine === seq) checking.value = false
    }
  }, 300)
})

const cleaned = computed(() => preview.value?.cleaned ?? '')
const valid = computed(() => Boolean(preview.value?.valid) && cleaned.value.length > 0)
const reason = computed(() => {
  if (err.value) return err.value
  if (!text.value.trim()) return '输入目录名，多级用 / 分隔；也可以敲几个字从已有目录里挑'
  if (checking.value) return '正在检查…'
  if (preview.value && !preview.value.valid) return preview.value.reason ?? '这个名字不可用'
  if (isExisting.value) return '这个目录已经有了，回车直接归到这里'
  if (preview.value && preview.value.cleaned !== text.value.trim()) {
    return '会按这个名称创建（非法字符已替换）'
  }
  return '按回车确认'
})

/** 最终路径：父/子 中的每一段都已清洗 */
const finalPath = computed(() => {
  if (!valid.value) return ''
  return cleaned.value
})

const dirty = computed(
  () => preview.value !== null && preview.value.cleaned !== text.value.trim(),
)

function submit() {
  if (!valid.value) return
  emit('confirm', cleaned.value)
}

function pick(p: string) {
  text.value = p
  listOpen.value = false
  active.value = -1
  void nextTick(() => inputEl.value?.focus())
}

function cancel() {
  open.value = false
  emit('cancel')
}

function onKey(e: KeyboardEvent) {
  if (listOpen.value && matches.value.length > 0) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      active.value = (active.value + 1) % matches.value.length
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      active.value = active.value <= 0 ? matches.value.length - 1 : active.value - 1
      return
    }
    if (e.key === 'Tab' || (e.key === 'Enter' && active.value >= 0)) {
      e.preventDefault()
      e.stopPropagation()
      pick(matches.value[active.value >= 0 ? active.value : 0])
      return
    }
  }
  if (e.key === 'Enter') {
    e.preventDefault()
    e.stopPropagation()
    submit()
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    if (listOpen.value) {
      listOpen.value = false
      active.value = -1
      return
    }
    cancel()
  }
  // 其余按键不拦：这里要能打出 s / p / n
}

defineExpose({ focus: () => inputEl.value?.focus() })
</script>

<template>
  <div class="wrap">
    <div class="field">
      <div class="picker">
        <input
          ref="inputEl"
          v-model="text"
          class="input"
          type="text"
          :placeholder="placeholder"
          :aria-invalid="Boolean(preview && !preview.valid)"
          :aria-expanded="listOpen"
          autocomplete="off"
          spellcheck="false"
          @keydown="onKey"
          @focus="listOpen = props.withSuggestions && dirs.count > 0"
        />
        <ul v-if="listOpen && dirs.count > 0" class="drop">
          <li v-if="matches.length === 0" class="empty">没有匹配的目录</li>
          <li
            v-for="(p, i) in matches"
            :key="p"
            class="opt"
            :class="{ on: i === active }"
            :title="p"
            @mousedown.prevent="pick(p)"
            @mouseenter="active = i"
          >
            <span class="opath">{{ p }}</span>
            <span v-if="p === query" class="otag">已有</span>
          </li>
        </ul>
      </div>
      <button class="btn primary" :disabled="!valid" @click="submit">
        {{ isExisting ? '归到这里' : '建这个目录' }}
      </button>
      <button class="btn" @click="cancel">取消</button>
    </div>

    <p class="hint" :class="{ bad: preview && !preview.valid, ok: valid && dirty && !isExisting }">
      <template v-if="isExisting">
        已有目录：<span class="mono">{{ query }}</span>，回车把这份归进去
      </template>
      <template v-else-if="valid && finalPath">
        将创建：<span class="mono">{{ finalPath }}</span>
        <template v-if="dirty">（已从「{{ text.trim() }}」清洗）</template>
      </template>
      <template v-else>{{ reason }}</template>
    </p>
  </div>
</template>

<style scoped>
.wrap {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.field {
  display: flex;
  align-items: center;
  gap: 8px;
}

.picker {
  position: relative;
  flex: 1;
  min-width: 0;
}

.input {
  width: 100%;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--r-field);
  border: 1px solid var(--c-border-strong);
  background: var(--c-field);
  font-size: 13px;
  font-family: var(--font);
  color: var(--c-text);
}
.input:focus {
  outline: none;
  border-color: var(--c-primary);
  box-shadow: 0 0 0 3px var(--c-primary-glow);
  background: var(--c-surface);
}

/* 自己画的下拉：datalist 在 WebView2 中文输入下基本用不了 */
.drop {
  position: absolute;
  top: 36px;
  left: 0;
  right: 0;
  z-index: 20;
  margin: 0;
  padding: 4px;
  list-style: none;
  max-height: 232px;
  overflow: auto;
  background: var(--c-surface);
  border: 0.5px solid var(--c-border);
  border-radius: var(--r-card);
  box-shadow: 0 10px 28px rgba(0, 0, 0, 0.14);
}
.opt {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: var(--r-item);
  font-size: 12px;
  cursor: default;
}
.opt.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
}
.opath {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.otag {
  flex: 0 0 auto;
  margin-left: auto;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: var(--r-pill);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
}
.empty {
  padding: 6px 8px;
  font-size: 12px;
  color: var(--c-text-3);
}

.btn {
  border: 0;
  border-radius: var(--r-btn);
  padding: 7px 12px;
  font-size: 12px;
  background: var(--c-field);
  color: var(--c-text);
  white-space: nowrap;
}
.btn.primary {
  background: var(--c-primary);
  color: #fff;
  box-shadow: var(--sh-primary);
}
.btn:disabled {
  opacity: 0.5;
}

.hint {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
  min-height: 18px;
}
.hint.bad {
  color: var(--c-danger);
}
.hint.ok {
  color: var(--c-warning-deep);
}

.mono {
  font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
  font-size: 12px;
}
</style>
