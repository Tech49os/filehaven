<script setup lang="ts">
/**
 * 新建目录输入框（T6.11）
 *
 * 输入 `父/子`，300ms 防抖后调后端 `preview_dir_name` 拿清洗结果，
 * 清洗后为空就禁用确认并说明原因 —— 用户改的是「写什么」，
 * 系统告诉用户「会变成什么」，不偷偷改完不吭声。
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
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

let timer: ReturnType<typeof setTimeout> | null = null
let seq = 0

/** 输入框是否打开 —— 供 App 层判断要不要抢键盘 */
const open = ref(true)

onMounted(() => {
  inputEl.value?.focus()
  emit('input-open', true)
  if (props.withSuggestions && dirs.count === 0) void dirs.load()
})

onUnmounted(() => {
  if (timer) clearTimeout(timer)
  emit('input-open', false)
})

watch(text, (v) => {
  err.value = null
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
  if (!text.value.trim()) return '输入目录名，多级用 / 分隔'
  if (checking.value) return '正在检查…'
  if (preview.value && !preview.value.valid) return preview.value.reason ?? '这个名字不可用'
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

function cancel() {
  open.value = false
  emit('cancel')
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    e.stopPropagation()
    submit()
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    cancel()
  }
  // 其余按键不拦：这里要能打出 s / p / n
}

defineExpose({ focus: () => inputEl.value?.focus() })
</script>

<template>
  <div class="wrap">
    <div class="field">
      <input
        ref="inputEl"
        v-model="text"
        class="input"
        type="text"
        :placeholder="placeholder"
        :list="withSuggestions ? 'dir-suggestions' : undefined"
        :aria-invalid="Boolean(preview && !preview.valid)"
        spellcheck="false"
        @keydown="onKey"
      />
      <datalist v-if="withSuggestions" id="dir-suggestions">
        <option v-for="p in dirs.allPaths" :key="p" :value="p" />
      </datalist>
      <button class="btn primary" :disabled="!valid" @click="submit">建这个目录</button>
      <button class="btn" @click="cancel">取消</button>
    </div>

    <p class="hint" :class="{ bad: preview && !preview.valid, ok: valid && dirty }">
      <template v-if="valid && finalPath">
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

.input {
  flex: 1;
  min-width: 0;
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
