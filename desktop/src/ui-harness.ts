import { createApp, h } from 'vue'
import { createPinia } from 'pinia'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import 'element-plus/theme-chalk/dark/css-vars.css'
import './styles/theme.css'
import EditorTools from './components/toolbox/EditorTools.vue'
import { setLanguage } from './i18n'

setLanguage('zh')
;(window as unknown as { __setLang: (l: 'zh' | 'en') => void }).__setLang = setLanguage
createApp({ render: () => h('div', { style: 'padding:16px;max-width:1500px' }, [h(EditorTools)]) })
  .use(createPinia())
  .use(ElementPlus, { locale: zhCn })
  .mount('#app')
