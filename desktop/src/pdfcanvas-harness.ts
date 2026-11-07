import { createApp, h } from 'vue'
import ElementPlus from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import 'element-plus/dist/index.css'
import './styles/theme.css'
import PdfCanvas from './components/PdfCanvas.vue'

// PDF 版面画布的浏览器测试用：只挂载画布，后端由测试注入的假 Tauri 回答。
const app = createApp({ render: () => h('div', { style: 'height:100vh;display:flex;flex-direction:column' }, [h(PdfCanvas, { path: '/tmp/a.pdf', target: '简体中文', ocr: 'vision' })]) })
app.use(ElementPlus, { locale: zhCn })
app.mount('#app')
