<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppIcon from './AppIcon.vue'
import { useAppStore } from '../stores/app'
import { onFilesFound } from '../core/events'
import type { UnlistenFn } from '@tauri-apps/api/event'

const route = useRoute()
const router = useRouter()
const app = useAppStore()

interface NavItem {
  path: string
  label: string
  icon: string
  badge?: number
  /** 角标为 0 时也显示（待迁移/待定区需要让用户看到 0） */
  alwaysBadge?: boolean
  /** 信息型角标：灰底不抢注意力（整理台是常驻工作量，不是待办告警） */
  soft?: boolean
}

const items = computed<NavItem[]>(() => [
  {
    path: '/workspace',
    label: '整理台',
    icon: 'dashboard',
    // 「还有多少文件待整理」原来只写在整理台里的一句「池里还有 N 份」，
    // 人不在那个页面就看不到。挂到侧栏，任何页面都知道还剩多少
    badge: app.pendingTotal,
    alwaysBadge: true,
    soft: true,
  },
  {
    path: '/migration',
    label: '待迁移',
    icon: 'layers',
    badge: app.pendingMigration,
    alwaysBadge: true,
  },
  { path: '/browser', label: '浏览', icon: 'folder' },
  {
    path: '/parked',
    label: '待定区',
    icon: 'clock',
    badge: app.parkedCount,
    alwaysBadge: true,
  },
  { path: '/settings', label: '设置', icon: 'settings' },
])

const isOn = (p: string) => route.path === p || route.path.startsWith(`${p}/`)

function go(p: string) {
  if (route.path !== p) void router.push(p)
}

const model = computed(() => app.modelStatus)
const modelTip = computed(() => {
  if (model.value.reachable) return `模型已就绪：${model.value.model ?? '未知'}`
  return '模型未连接，目录建议会降级为手动'
})

/** 侧栏常驻，事件订阅挂这儿最稳：切页面不会漏掉「又来了 N 份」 */
let unlisten: UnlistenFn | null = null
onMounted(async () => {
  unlisten = await onFilesFound((p) => app.noteNewFiles(p.added))
})
onUnmounted(() => unlisten?.())

/** 点一下：刷新计数（新文件得进池才看得见）+ 收掉提示 + 去整理台 */
function goNewFiles() {
  app.dismissNewFiles()
  void app.refreshBoot()
  go('/workspace')
}
</script>

<template>
  <aside class="side glass-side">
    <div class="group-title">资料库</div>
    <nav>
      <button
        v-for="it in items"
        :key="it.path"
        class="nav"
        :class="{ on: isOn(it.path) }"
        :aria-current="isOn(it.path) ? 'page' : undefined"
        :title="it.soft ? `还有 ${it.badge ?? 0} 份没定去向` : undefined"
        @click="go(it.path)"
      >
        <AppIcon :name="it.icon" :size="16" />
        <span class="lbl">{{ it.label }}</span>
        <span
          v-if="it.alwaysBadge || (it.badge ?? 0) > 0"
          class="badge"
          :class="{ zero: (it.badge ?? 0) === 0, soft: it.soft }"
          >{{ it.badge ?? 0 }}</span
        >
      </button>
    </nav>

    <!-- 监听/增量扫描发现了新文件（T3.7）。灰色中性条，不闪不红：
         这不是错误，是「又来了活儿」，点一下去处理 -->
    <button v-if="app.newFiles > 0" class="newfiles" :title="'点一下去整理台'" @click="goNewFiles">
      <AppIcon name="sparkle" :size="12" />
      <span class="ellipsis">+{{ app.newFiles }} 份新文件</span>
    </button>

    <div class="foot">
      <div class="root">
        <AppIcon name="drive" :size="14" />
        <div class="path">
          <div class="p1 ellipsis">{{ app.archiveRoot ?? '未设置归档区' }}</div>
          <div class="p2 ellipsis">{{ app.sourceRoots.length }} 个源区</div>
        </div>
      </div>
      <div class="model" :title="modelTip">
        <i class="dot" :class="{ on: model.reachable }" />
        <span class="mtext ellipsis">{{ modelTip }}</span>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.side {
  width: 200px;
  flex: 0 0 200px;
  /* 材质背景与右侧分隔线由 glass.css 的 .glass-side 提供 */
  display: flex;
  flex-direction: column;
  padding: 10px 10px 0;
}

/* 窄窗时侧栏也让一步：省下来的 24px 直接给中间的卡片 */
@media (max-width: 1240px) {
  .side {
    width: 176px;
    flex-basis: 176px;
    padding: 8px 8px 0;
  }
}

.group-title {
  font-size: 11px;
  font-weight: 500;
  letter-spacing: 0.3px;
  color: var(--c-text-2);
  padding: 6px 8px 8px;
}

nav {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.nav {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 10px;
  border-radius: var(--r-item);
  font-size: 13px;
  color: var(--c-text);
  text-align: left;
  transition: background 0.12s ease;
  min-height: 32px;
  background: transparent;
  border: 0;
  cursor: default;
}
.nav:hover {
  background: var(--c-hover);
}
.nav.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-weight: 500;
}
.nav:focus-visible {
  outline: 2px solid var(--c-primary);
  outline-offset: 1px;
  box-shadow: 0 0 0 3px var(--c-primary-glow);
}

.lbl {
  flex: 1;
}

.badge {
  background: var(--c-danger);
  color: #fff;
  font-size: 11px;
  font-weight: 500;
  border-radius: 10px;
  min-width: 18px;
  height: 18px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.badge.zero {
  background: rgba(0, 0, 0, 0.08);
  color: var(--c-text-2);
}
/* 整理台的角标是「还剩多少活」的常驻数字，不是催办：
   灰底、不加粗外扩，读完心里有数就行 */
.badge.soft {
  background: rgba(0, 0, 0, 0.06);
  color: var(--c-text-2);
  font-weight: 400;
}

/* 新文件提示：一格灰色小条，带一点点强调色图标，读完就知道有活儿 */
.newfiles {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  padding: 6px 9px;
  border: 0;
  border-radius: var(--r-item);
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-size: 12px;
  text-align: left;
  cursor: default;
  min-width: 0;
}
.newfiles:hover {
  filter: brightness(0.97);
}
.newfiles:focus-visible {
  outline: 2px solid var(--c-primary);
  outline-offset: 1px;
}

.foot {
  margin-top: auto;
  border-top: 0.5px solid var(--c-border);
  padding: 12px 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.root {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--c-text-2);
}

.path {
  min-width: 0;
}
.p1 {
  font-size: 12px;
  font-weight: 500;
  color: var(--c-text);
}
.p2 {
  font-size: 11px;
  color: var(--c-text-3);
}

.model {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--c-text-3);
  flex: 0 0 7px;
}
.dot.on {
  background: var(--c-success);
  box-shadow: 0 0 0 3px var(--c-success-bg);
}
.mtext {
  font-size: 11px;
  color: var(--c-text-2);
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
