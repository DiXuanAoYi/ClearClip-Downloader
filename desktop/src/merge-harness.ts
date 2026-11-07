import { createApp, h, type Component } from 'vue'
import { createPinia } from 'pinia'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import './styles/theme.css'
import FloatApp from './FloatApp.vue'
import ExcerptsPanel from './components/ExcerptsPanel.vue'
import ScreenView from './views/ScreenView.vue'
import AiTools from './components/toolbox/AiTools.vue'
import GlossaryPanel from './components/GlossaryPanel.vue'
import { useAppStore } from './stores/app'
import { api } from './api'
import { setLanguage } from './i18n'

// 合并后的几块界面（悬浮窗、摘录、屏幕翻译历史、工具箱 AI）的浏览器测试用：按网址 # 挂载一个，后端由测试注入的假 Tauri 回答。
setLanguage('zh')
const pinia = createPinia()
const which = location.hash.slice(1)
const C: Record<string, Component> = { float: FloatApp, excerpts: ExcerptsPanel, screen: ScreenView, ai: AiTools, glossary: GlossaryPanel }
const store = useAppStore(pinia)
store.settings = (await api.getSettings()) as never
;(window as unknown as { __app: typeof store }).__app = store
const style = which === 'float' ? 'width:780px;height:64px;position:relative' : 'padding:16px;max-width:1100px;height:900px;display:flex;flex-direction:column'
createApp({ render: () => h('div', { style }, [which === 'glossary' ? h(GlossaryPanel, { modelValue: store.settings!.ai, 'onUpdate:modelValue': () => {} }) : h(C[which] ?? ExcerptsPanel)]) })
  .use(pinia)
  .use(ElementPlus, { locale: zhCn })
  .mount('#app')
