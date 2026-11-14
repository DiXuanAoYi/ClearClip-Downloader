import { createApp, defineComponent, h, type Component } from 'vue'
import { createPinia } from 'pinia'
import { ElConfigProvider } from 'element-plus'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import '@fontsource/chakra-petch/latin-500.css'
import '@fontsource/chakra-petch/latin-600.css'
import '@fontsource/chakra-petch/latin-700.css'
import '@fontsource/jetbrains-mono/latin-400.css'
import '@fontsource/jetbrains-mono/latin-500.css'
import './styles/theme.css'
import './styles/skin.css'
import App from './App.vue'
import { elLocale } from './i18n/elLocale'
import { installGlobalErrorHandlers } from './utils/globalErrors'

// 迷你窗、悬浮拖拽窗与主窗口共用同一份前端，按 hash 区分。非主窗口的根组件按需加载，主窗口不再带上它们的代码。
const roots: Record<string, () => Promise<{ default: Component }>> = {
  '#mini': () => import('./MiniApp.vue'),
  '#float': () => import('./FloatApp.vue'),
  '#write': () => import('./WriteApp.vue'),
  '#wbadge': () => import('./WriteBadge.vue'),
  '#shot': () => import('./ShotApp.vue'),
  '#shotpin': () => import('./ShotPinApp.vue'),
  '#liveov': () => import('./LiveOverlayApp.vue'),
  '#livesub': () => import('./LiveSubApp.vue'),
  '#livepick': () => import('./LivePickApp.vue'),
}

async function boot() {
  const root = roots[location.hash] ? (await roots[location.hash]()).default : App
  // Element Plus 自带文案跟随界面语言（useLanguage 里改 elLocale）
  const Shell = defineComponent({ render: () => h(ElConfigProvider, { locale: elLocale.value }, () => h(root)) })
  const app = createApp(Shell).use(createPinia())
  installGlobalErrorHandlers(app)
  app.mount('#app')
}

void boot()
