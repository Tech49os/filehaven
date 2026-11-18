<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppIcon from './AppIcon.vue'
import { useDataStore } from '../stores/data'

const route = useRoute()
const router = useRouter()
const data = useDataStore()

const items = computed(() => [
  { path: '/main/dashboard', label: '概览', icon: 'dashboard' },
  { path: '/main/browse', label: '分类浏览', icon: 'folder' },
  {
    path: '/main/pending',
    label: '待确认',
    icon: 'clock',
    badge: data.pendingFiles.length,
  },
  { path: '/main/logs', label: '迁移记录', icon: 'list' },
  { path: '/main/settings', label: '设置', icon: 'settings' },
])

function go(p: string) {
  router.push(p)
}
</script>

<template>
  <aside class="side">
    <div class="group-title">资料库</div>
    <nav>
      <button
        v-for="it in items"
        :key="it.path"
        class="nav"
        :class="{ on: route.path === it.path }"
        @click="go(it.path)"
      >
        <AppIcon :name="it.icon" :size="16" />
        <span class="lbl">{{ it.label }}</span>
        <span v-if="it.badge" class="badge">{{ it.badge }}</span>
      </button>
    </nav>

    <div class="foot">
      <div class="root">
        <AppIcon name="drive" :size="14" />
        <div class="path">
          <div class="p1 ellipsis">{{ data.settings.rootName }}</div>
          <div class="p2 ellipsis">{{ data.settings.rootPath }}</div>
        </div>
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
  backdrop-filter: blur(22px);
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
}
.nav:hover {
  background: var(--c-hover);
}
.nav.on {
  background: var(--c-primary-bg);
  color: var(--c-primary);
  font-weight: 500;
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

.foot {
  margin-top: auto;
  border-top: 0.5px solid var(--c-border);
  padding: 12px 8px 14px;
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
</style>
