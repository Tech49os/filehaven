<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppIcon from './AppIcon.vue'
import { useAppStore } from '../stores/app'

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
}

const items = computed<NavItem[]>(() => [
  { path: '/workspace', label: '整理台', icon: 'dashboard' },
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
</script>

<template>
  <aside class="side">
    <div class="group-title">资料库</div>
    <nav>
      <button
        v-for="it in items"
        :key="it.path"
        class="nav"
        :class="{ on: isOn(it.path) }"
        :aria-current="isOn(it.path) ? 'page' : undefined"
        @click="go(it.path)"
      >
        <AppIcon :name="it.icon" :size="16" />
        <span class="lbl">{{ it.label }}</span>
        <span
          v-if="it.alwaysBadge || (it.badge ?? 0) > 0"
          class="badge"
          :class="{ zero: (it.badge ?? 0) === 0 }"
          >{{ it.badge ?? 0 }}</span
        >
      </button>
    </nav>

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
  background: var(--c-sidebar);
  border-right: 0.5px solid var(--c-border);
  display: flex;
  flex-direction: column;
  padding: 10px 10px 0;
  backdrop-filter: blur(30px) saturate(180%);
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
