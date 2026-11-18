<script setup lang="ts">
/**
 * 文件清单（带版本族折叠）
 *
 * 同一个文档的多个版本只占一行：`▸ 议案  [5 个版本] 最新 2.1MB · 08-25`
 * 点开才列出每个版本的原名 / 大小 / 时间。真实数据里 30% 的文件是版本族成员，
 * 不折叠的话一个分类展开就是六七行几乎一样的名字，用户没法判断该用哪份。
 */
import { computed, ref } from 'vue'
import { foldFamilies, hasVersionMark, type FoldRow } from '../../core/version'
import { useWizardStore } from '../../stores/wizard'

const props = withDefaults(defineProps<{ ids: string[]; showPath?: boolean }>(), {
  // 注意：布尔 prop 不传时 Vue 会给 false，不是 undefined——必须显式给默认值
  showPath: true,
})
const w = useWizardStore()

/** 展开了哪些族（按族键记，删类重算后不会错位） */
const open = ref(new Set<string>())

const rows = computed<FoldRow[]>(() =>
  foldFamilies(
    props.ids.map((id) => ({
      id,
      name: w.fileIndex[id] ?? '',
      size: w.fileMeta[id]?.size ?? 0,
      mtime: w.fileMeta[id]?.mtime ?? 0,
    }))
  )
)

function toggle(key: string) {
  const next = new Set(open.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  open.value = next
}

function fmtSize(n: number): string {
  if (!n) return '—'
  if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))}KB`
  return `${(n / 1024 / 1024).toFixed(1)}MB`
}

function fmtDate(ms: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  return `${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}
</script>

<template>
  <div class="fl">
    <template v-for="r in rows" :key="r.latest.id">
      <!-- 单份：和以前一样 -->
      <div v-if="!r.family" class="fl-row">
        <span class="fl-n">{{ w.fileIndex[r.latest.id] ?? '（已不在扫描结果里）' }}</span>
        <span v-if="w.dupIndex[r.latest.id]?.length" class="fl-dup">
          与另 {{ w.dupIndex[r.latest.id].length }} 份内容相同
        </span>
        <span v-if="showPath" class="fl-p">{{ w.filePathIndex[r.latest.id] }}</span>
      </div>

      <!-- 版本族：一行，点开看全部 -->
      <div v-else class="fl-fam">
        <div
          class="fl-head"
          role="button"
          tabindex="0"
          @click="toggle(r.family.key)"
          @keydown.enter.prevent="toggle(r.family.key)"
        >
          <span class="fl-caret" :class="{ on: open.has(r.family.key) }">›</span>
          <span class="fl-n">{{ r.family.title }}</span>
          <span class="fl-badge">
            {{ r.family.files.length }}{{ hasVersionMark(r.family) ? ' 个版本' : ' 份同名' }}
          </span>
          <span class="fl-meta">最新 {{ fmtSize(r.latest.size) }} · {{ fmtDate(r.latest.mtime) }}</span>
        </div>

        <div v-if="open.has(r.family.key)" class="fl-kids">
          <div v-for="f in r.family.files" :key="f.id" class="fl-kid">
            <span class="fl-kn">{{ w.fileIndex[f.id] ?? '（已不在扫描结果里）' }}</span>
            <span v-if="w.dupIndex[f.id]?.length" class="fl-dup">
              与另 {{ w.dupIndex[f.id].length }} 份内容相同
            </span>
            <span class="fl-km">{{ fmtSize(f.size) }} · {{ fmtDate(f.mtime) }}</span>
            <span class="fl-tag">{{ f.id === r.family.latestId ? '最新' : '旧版' }}</span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.fl {
  display: flex;
  flex-direction: column;
}
.fl-row {
  display: flex;
  gap: 12px;
  align-items: baseline;
  padding: 3px 4px;
  border-radius: 5px;
  min-width: 0;
}
.fl-row:hover {
  background: var(--c-hover);
}
.fl-n {
  flex: 1;
  min-width: 0;
  font-size: 12.5px;
  color: var(--c-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fl-p {
  max-width: 46%;
  font-size: 11px;
  color: var(--c-text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: rtl;
  text-align: left;
}

/* 版本族 */
.fl-fam {
  border-radius: 6px;
}
.fl-head {
  display: flex;
  gap: 8px;
  align-items: baseline;
  padding: 3px 4px;
  border-radius: 5px;
  cursor: pointer;
  min-width: 0;
}
.fl-head:hover {
  background: var(--c-hover);
}
.fl-caret {
  flex: 0 0 auto;
  width: 10px;
  font-size: 11px;
  color: var(--c-text-3);
  transition: transform 0.12s ease;
  transform-origin: 50% 50%;
}
.fl-caret.on {
  transform: rotate(90deg);
}
.fl-badge {
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--c-text-2);
  background: var(--c-hover);
  border-radius: 4px;
  padding: 1px 5px;
}
.fl-meta {
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}
.fl-kids {
  margin: 1px 0 3px 18px;
  padding-left: 8px;
  border-left: 1px solid var(--c-border);
  display: flex;
  flex-direction: column;
}
.fl-kid {
  display: flex;
  gap: 8px;
  align-items: baseline;
  padding: 2px 4px;
  min-width: 0;
}
.fl-kn {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--c-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fl-km {
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--c-text-3);
  font-variant-numeric: tabular-nums;
}
.fl-tag {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--c-text-3);
  border: 0.5px solid var(--c-border);
  border-radius: 3px;
  padding: 0 3px;
}
.fl-dup {
  flex: 0 0 auto;
  font-size: 10px;
  color: var(--c-warning-deep);
  background: var(--c-warning-soft, rgba(255, 159, 10, 0.12));
  border-radius: 3px;
  padding: 0 4px;
}
</style>
