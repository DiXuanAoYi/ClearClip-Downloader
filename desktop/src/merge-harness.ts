import { createApp, h, type Component } from 'vue'
import { createPinia } from 'pinia'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import './styles/theme.css'
import FloatApp from './FloatApp.vue'
import ExcerptsPanel from './components/ExcerptsPanel.vue'
import ScreenView from './views/ScreenView.vue'
import AiTools from './components/toolbox/AiTools.vue'
import GlossaryPanel from './components/GlossaryPanel.vue'
import VideoTools from './components/toolbox/VideoTools.vue'
import ParseView from './views/ParseView.vue'
import FileTransView from './views/FileTransView.vue'
import WriteApp from './WriteApp.vue'
import TransferPanel from './components/TransferPanel.vue'
import { useAppStore, useParseStore } from './stores/app'
import { api } from './api'
import { setLanguage } from './i18n'

// 合并后的几块界面（悬浮窗、摘录、屏幕翻译历史、工具箱 AI、工具箱快速处理）、新版解析结果页和写作窗的浏览器测试用：按网址 # 挂载一个，后端由测试注入的假 Tauri 回答。
setLanguage('zh')
const pinia = createPinia()
const which = location.hash.slice(1)
const C: Record<string, Component> = { write: WriteApp, parse: ParseView, float: FloatApp, excerpts: ExcerptsPanel, screen: ScreenView, ai: AiTools, glossary: GlossaryPanel, filetr: FileTransView, transfer: TransferPanel }
const store = useAppStore(pinia)
store.settings = (await api.getSettings()) as never
;(window as unknown as { __app: typeof store }).__app = store
// 解析页：测试把解析结果放在 window.__result，暗色主题用 window.__dark
const W = window as unknown as { __result?: unknown; __dark?: boolean; __parse?: unknown }
if (W.__dark) document.documentElement.classList.add('dark')
if (which === 'parse') {
  const ps = useParseStore(pinia)
  ps.text = 'https://example.test/x'
  ps.result = (W.__result ?? null) as never
  W.__parse = ps
}
const style = which === 'float' ? 'width:780px;height:64px;position:relative' : which === 'write' ? 'width:464px' : 'padding:16px;max-width:1100px;height:900px;display:flex;flex-direction:column'
createApp({ render: () => h('div', { style }, [which === 'glossary' ? h(GlossaryPanel, { modelValue: store.settings!.ai, 'onUpdate:modelValue': () => {} }) : which === 'video' ? h(VideoTools, { preset: ['/m/clip.mp4'] }) : h(C[which] ?? ExcerptsPanel)]) })
  .use(pinia)
  .use(ElementPlus, { locale: zhCn })
  .mount('#app')
