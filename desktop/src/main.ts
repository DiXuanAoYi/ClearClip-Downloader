import { createApp } from 'vue'
import { createPinia } from 'pinia'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import './styles/theme.css'
import App from './App.vue'
import MiniApp from './MiniApp.vue'
import FloatApp from './FloatApp.vue'
import WriteApp from './WriteApp.vue'
import WriteBadge from './WriteBadge.vue'
import ShotApp from './ShotApp.vue'
import LiveOverlayApp from './LiveOverlayApp.vue'
import LiveBarApp from './LiveBarApp.vue'
import LiveSubApp from './LiveSubApp.vue'
import LivePickApp from './LivePickApp.vue'

// 迷你窗、悬浮拖拽窗与主窗口共用同一份前端，按 hash 区分。
const root = location.hash === '#mini' ? MiniApp : location.hash === '#float' ? FloatApp : location.hash === '#write' ? WriteApp : location.hash === '#wbadge' ? WriteBadge : location.hash === '#shot' ? ShotApp : location.hash === '#liveov' ? LiveOverlayApp : location.hash === '#livebar' ? LiveBarApp : location.hash === '#livesub' ? LiveSubApp : location.hash === '#livepick' ? LivePickApp : App

createApp(root).use(createPinia()).use(ElementPlus, { locale: zhCn }).mount('#app')
