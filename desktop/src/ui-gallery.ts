// 组件陈列页（开发 / e2e 用，不进安装包）：浅 / 深色下展示 components/ui 里所有组件的所有状态。
// 网址：/ui-gallery.html ，加 #dark 为深色；window.__setDark(true|false) 可在页面里切换。
import { createApp } from 'vue'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import '@fontsource/chakra-petch/latin-500.css'
import '@fontsource/jetbrains-mono/latin-400.css'
import './styles/theme.css'
import './styles/skin.css'
import UiGallery from './components/ui/UiGallery.vue'

const setDark = (d: boolean) => document.documentElement.classList.toggle('dark', d)
;(window as unknown as { __setDark: (d: boolean) => void }).__setDark = setDark
setDark(location.hash.includes('dark'))
createApp(UiGallery).use(ElementPlus, { locale: zhCn }).mount('#app')
