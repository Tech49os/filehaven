<script setup lang="ts">
import { computed } from 'vue'
import AppWindow from '../../components/AppWindow.vue'
import WizardStepper from '../../components/WizardStepper.vue'
import { useWizardStore } from '../../stores/wizard'
import WelcomeStep from './WelcomeStep.vue'
import ScanStep from './ScanStep.vue'
import InductionStep from './InductionStep.vue'
import ReviewStep from './ReviewStep.vue'
import MigrateStep from './MigrateStep.vue'
import DoneStep from './DoneStep.vue'

const w = useWizardStore()

const comp = computed(() => {
  const map = [
    WelcomeStep,
    WelcomeStep,
    ScanStep,
    InductionStep,
    ReviewStep,
    MigrateStep,
    MigrateStep,
    DoneStep,
  ]
  return map[w.step] ?? WelcomeStep
})
</script>

<template>
  <AppWindow title="初始化向导">
    <div class="wiz">
      <div class="wiz-top">
        <WizardStepper :step="w.step" :name="w.stepName" />
      </div>
      <div class="wiz-body">
        <component :is="comp" />
      </div>
    </div>
  </AppWindow>
</template>

<style scoped>
.wiz {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.wiz-top {
  flex: 0 0 auto;
  padding: 12px 24px;
  border-bottom: 0.5px solid var(--c-border);
  background: var(--c-titlebar);
}
.wiz-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
</style>
