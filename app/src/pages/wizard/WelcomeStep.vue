<script setup lang="ts">
import AppIcon from '../../components/AppIcon.vue'
import { useWizardStore } from '../../stores/wizard'

const w = useWizardStore()

async function pick() {
  const ok = await w.selectRealRoot()
  if (ok) w.goto(2)
}

function demo() {
  w.useDemo()
  w.goto(2)
}

async function changeArchive() {
  await w.pickArchiveRoot()
}
</script>

<template>
  <div class="step">
    <div class="step-body scrollable">
      <div class="hero">
        <span class="pill">
          <AppIcon name="shield" :size="12" />
          本地处理 · 不联网 · 原文件不直删
        </span>
        <h1>
          把散落各处的文件<br />
          收进一个地方
        </h1>
        <p class="lead">
          扫描 → AI 归纳分类 → 你确认 → 安全迁移。<br />
          不用先想好分类，系统会自己看出来你有哪些类东西。
        </p>
      </div>

      <div class="features">
        <div class="f">
          <div class="fi">
            <AppIcon name="sparkle" :size="17" />
          </div>
          <h4>AI 自动归纳</h4>
          <p>扫描后自动发现你的分类体系并命名，你只需微调</p>
        </div>
        <div class="f">
          <div class="fi">
            <AppIcon name="layers" :size="17" />
          </div>
          <h4>多维归档</h4>
          <p>工作域 · 文档类型 · 年份，任一维度都能筛出来</p>
        </div>
        <div class="f">
          <div class="fi">
            <AppIcon name="shield" :size="17" />
          </div>
          <h4>三段式安全</h4>
          <p>复制 → 校验哈希 → 源进隔离区，全程可撤销</p>
        </div>
      </div>

      <div class="dest">
        <AppIcon name="drive" :size="16" />
        <span class="k">归档到</span>
        <code class="v">{{ w.archivePath }}</code>
        <button v-if="w.desktop" class="link" @click="changeArchive">更改</button>
        <span class="fill" />
        <span class="muted tiny">{{
          w.mode === 'real' ? `来自 ${w.sourceLabel}` : '尚未选择要整理的目录'
        }}</span>
      </div>

      <div v-if="w.error" class="err">{{ w.error }}</div>
    </div>

    <div class="step-foot">
      <span class="muted tiny">系统目录（Windows / Program Files / AppData 等）自动排除，永不扫描</span>
      <div class="spacer" />
      <button class="btn btn-secondary" @click="demo">用示例数据体验</button>
      <button class="btn btn-primary" @click="pick">选择要整理的目录</button>
    </div>
  </div>
</template>

<style scoped>
.hero {
  padding: 38px 0 8px;
  text-align: center;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border-radius: var(--r-pill);
  background: var(--c-success-bg);
  color: var(--c-success-deep);
  font-size: 11.5px;
  font-weight: 500;
}
h1 {
  font-size: 30px;
  font-weight: 500;
  letter-spacing: -0.6px;
  line-height: 1.28;
  margin: 16px 0 12px;
}
.lead {
  font-size: 13.5px;
  color: var(--c-text-2);
  line-height: 1.7;
  margin: 0;
}

.features {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin-top: 24px;
}
.f {
  background: var(--c-surface);
  border-radius: var(--r-card);
  box-shadow: var(--sh-card);
  border: 0.5px solid var(--c-border);
  padding: 18px;
}
.fi {
  width: 30px;
  height: 30px;
  border-radius: 9px;
  background: var(--c-primary-bg);
  color: var(--c-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 10px;
}
h4 {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 500;
}
.f p {
  margin: 0;
  font-size: 12px;
  color: var(--c-text-2);
  line-height: 1.6;
}

.dest {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  background: rgba(0, 0, 0, 0.02);
  border-radius: var(--r-banner);
  color: var(--c-text-2);
  margin-top: auto;
}
.k {
  font-size: 12px;
  flex: 0 0 auto;
}
.v {
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 12px;
  color: var(--c-text);
  background: var(--c-surface);
  padding: 2px 8px;
  border-radius: 6px;
  border: 0.5px solid var(--c-border);
  max-width: 46%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.link {
  border: none;
  background: none;
  color: var(--c-primary);
  font-size: 12px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 5px;
}
.link:hover {
  background: var(--c-primary-bg);
}
.fill {
  flex: 1;
}
.err {
  margin-top: 10px;
  font-size: 12px;
  color: var(--c-danger-deep, #c1272d);
  background: var(--c-danger-bg, rgba(255, 59, 48, 0.1));
  padding: 8px 12px;
  border-radius: var(--r-banner);
}
</style>
