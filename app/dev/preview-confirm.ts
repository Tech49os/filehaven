/**
 * 开发用预览页：把「确认分类方案」单独拎出来在浏览器里看。
 *
 * 为什么需要：这是纯界面步骤，逻辑测试全绿也可能排版稀烂（上一版就是这样被用户
 * 指出「看不太清晰」）。开发时 `npm run dev` 打开
 *   http://127.0.0.1:5273/dev/preview-confirm.html
 * 就能用真实数据看效果，不用打包 Tauri、也不用真跑一遍扫描。
 *
 * 数据直接吃 test/ 下的真实扫描与模型结果（e2e-proposal.json / e2e-domain.json）。
 */
import { createApp, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import '../src/styles/tokens.css'
import '../src/styles/global.css'
import TopicConfirmStep from '../src/pages/wizard/TopicConfirmStep.vue'
import { useWizardStore } from '../src/stores/wizard'
import scan from '../test/desktop-scan.json'
import proposalJson from '../test/e2e-proposal.json'
import domainJson from '../test/e2e-domain.json'

const files = (scan as { files: { name: string; path: string }[] }).files
const fileIndex: Record<string, string> = {}
const filePathIndex: Record<string, string> = {}
files.forEach((f, i) => {
  fileIndex[`f${i}`] = f.name
  filePathIndex[`f${i}`] = f.path
})

const assign = (domainJson as { assign: Record<string, string> }).assign
const topics = (proposalJson as {
  proposal: { topics: { id: string; name: string; aliases: string[]; fileIds: string[]; samples: string[]; weak?: boolean; weakReason?: 'doc' | 'who' }[] }
}).proposal.topics.map((t) => ({ ...t, domain: assign[t.name] }))

const pinia = createPinia()
setActivePinia(pinia)
const w = useWizardStore()
w.proposal = { topics, ungroupedIds: (proposalJson as { proposal: { ungroupedIds: string[] } }).proposal.ungroupedIds }
w.fileIndex = fileIndex
w.filePathIndex = filePathIndex
w.domainSource = 'llm'

createApp({
  render: () =>
    h('div', { class: 'preview-win' }, [h(TopicConfirmStep)]),
})
  .use(pinia)
  .mount('#app')
