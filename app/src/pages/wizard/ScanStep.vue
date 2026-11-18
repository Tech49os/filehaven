<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AppIcon from '../../components/AppIcon.vue'
import { useWizardStore } from '../../stores/wizard'

const w = useWizardStore()
const done = ref(false)
const determinate = computed(() => w.mode === 'demo')

onMounted(async () => {
  await w.startScan()
  done.value = true
})

const pct = computed(() => {
  const p = w.progress
  if (!p.total) return 0
  return Math.min(100, Math.round((p.processed / p.total) * 100))
})

function cancel() {
  w.cancelled.cancelled = true
}

function next() {
  w.goto(3)
}
</script>

<template>
  <div class="step">
    <div class="step-body">
      <div class="head">
        <h1 v-if="!done">正在扫描文件</h1>
        <h1 v-else>扫描完成</h1>
        <p class="sub">
          {{
            done
              ? `在 ${w.sourceLabel} 中找到 ${w.scannedCount} 个可整理的文件`
              : `正在读取 ${w.sourceLabel} 的结构，系统目录已自动跳过`
          }}
        </p>
      </div>

      <div class="panel">
        <div class="bar-wrap">
          <div class="bar" :class="{ indet: !determinate && !done }">
            <div
              class="fill"
              :style="{ width: done ? '100%' : determinate ? pct + '%' : '38%' }"
            />
          </div>
          <div class="meta">
            <span class="muted tiny">
              {{ done ? '完成' : `已发现 ${w.progress.processed} 个文件` }}
            </span>
            <span v-if="w.progress.skipped" class="muted tiny" title="快捷方式、可执行程序、压缩包、系统文件读不出正文，收进来也归不了类">
              已跳过 {{ w.progress.skipped }} 个非文档
            </span>
          </div>
        </div>

        <div v-if="!done" class="current">
          <AppIcon name="folder" :size="13" />
          <span class="ellipsis">{{ w.progress.currentPath || '准备中…' }}</span>
        </div>

        <div v-else class="result">
          <div class="stat-row">
            <div class="s">
              <div class="n">{{ w.scannedCount }}</div>
              <div class="l">待整理文件</div>
            </div>
            <div class="s">
              <div class="n">{{ w.progress.skipped }}</div>
              <div class="l">已跳过（快捷方式/程序/压缩包/超大）</div>
            </div>
            <div class="s">
              <div class="n">{{ w.mode === 'demo' ? '示例' : '真实' }}</div>
              <div class="l">数据来源</div>
            </div>
          </div>
        </div>
      </div>

      <div v-if="w.error" class="notice">
        <AppIcon name="info" :size="14" />
        <span>{{ w.error }}</span>
      </div>

      <div class="tip">
        <AppIcon name="sparkle" :size="14" />
        <span>下一步 AI 会先归纳出你的分类体系，再逐个文件定位，拿不准的会交给你确认。</span>
      </div>
    </div>

    <div class="step-foot">
      <button v-if="!done" class="btn btn-muted" @click="cancel">取消扫描</button>
      <div class="spacer" />
      <button v-if="done" class="btn btn-primary" @click="next">
        开始 AI 归纳
        <AppIcon name="arrow-right" :size="14" />
      </button>
      <button v-else class="btn btn-primary" disabled>
        <span class="spinner" />
        扫描中…
      </button>
    </div>
  </div>
</template>

<style scoped>
.head h1 {
  font-size: 24px;
  font-weight: 500;
  letter-spacing: -0.4px;
  margin: 0;
}
.sub {
  font-size: 13px;
  color: var(--c-text-2);
  margin: 6px 0 0;
}

.panel {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  padding: 20px;
  margin-top: 4px;
}

.bar-wrap {
}
.bar {
  height: 6px;
  background: rgba(0, 0, 0, 0.06);
  border-radius: 4px;
  overflow: hidden;
}
.bar.indet .fill {
  animation: slide 1.1s ease-in-out infinite;
  transition: none;
}
@keyframes slide {
  0% {
    margin-left: -38%;
  }
  100% {
    margin-left: 100%;
  }
}
.fill {
  height: 100%;
  background: linear-gradient(90deg, #4da2ff, var(--c-primary));
  border-radius: 4px;
  transition: width 0.25s ease;
}
.meta {
  display: flex;
  justify-content: space-between;
  margin-top: 8px;
}

.current {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-top: 14px;
  padding: 9px 12px;
  background: var(--c-field);
  border-radius: var(--r-item);
  font-size: 12px;
  color: var(--c-text-2);
  min-width: 0;
}

.stat-row {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin-top: 4px;
}
.s {
  text-align: center;
  padding: 8px 0;
}
.n {
  font-size: 22px;
  font-weight: 500;
  letter-spacing: -0.3px;
}
.l {
  font-size: 11.5px;
  color: var(--c-text-2);
  margin-top: 2px;
}

.notice {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 14px;
  border-radius: var(--r-banner);
  background: var(--c-warning-bg);
  color: var(--c-warning-deep);
  font-size: 12px;
  line-height: 1.55;
}

.tip {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  font-size: 12px;
  color: var(--c-text-2);
  padding: 0 2px;
  line-height: 1.6;
  margin-top: auto;
}
</style>
