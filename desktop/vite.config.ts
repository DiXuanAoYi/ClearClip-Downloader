import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'

const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  // Element Plus 组件按需引入（只影响脚本）：模板里的 <el-xxx> 与指令由解析器自动导入。
  // 样式仍在 main.ts 里整份先加载：skin.css 对 .el-* 的覆盖依赖“Element 样式在前”，按需样式会随懒加载块后到而压过皮肤。
  plugins: [vue(), Components({ resolvers: [ElementPlusResolver({ importStyle: false })], dts: false })],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    target: 'es2021',
    chunkSizeWarningLimit: 1500,
  },
})
