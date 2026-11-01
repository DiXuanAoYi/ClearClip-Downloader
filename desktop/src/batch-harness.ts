import { createApp, h } from 'vue'
import { createPinia } from 'pinia'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import './styles/theme.css'
import BatchAddDialog from './components/BatchAddDialog.vue'
import QueueView from './views/QueueView.vue'
import BatchView from './views/BatchView.vue'
import { useAppStore, useQueueStore } from './stores/app'
import { api } from './api'
import { setLanguage } from './i18n'

// 批量添加面板、批量下载页和下载队列的浏览器测试用：只挂载这几个，后端由测试注入的假 Tauri 回答。
setLanguage('zh')
const pinia = createPinia()
const app = createApp({ render: () => h('div', { style: 'padding:16px;max-width:1100px' }, [h(BatchAddDialog), h(BatchView), h(QueueView)]) })
app.use(pinia).use(ElementPlus, { locale: zhCn })
const store = useAppStore(pinia)
store.settings = (await api.getSettings()) as never
void useQueueStore(pinia).start()
;(window as unknown as { __app: typeof store }).__app = store
app.mount('#app')
