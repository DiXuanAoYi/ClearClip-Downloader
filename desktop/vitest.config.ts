import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'

// 组件级测试（vitest + @vue/test-utils + jsdom）。与 e2e/unit 的 node 测试互不影响：
// 这里只收 tests/component/**，运行：npm run test:component。
export default defineConfig({
  plugins: [vue(), Components({ resolvers: [ElementPlusResolver({ importStyle: false })], dts: false })],
  test: {
    environment: 'jsdom',
    include: ['tests/component/**/*.test.ts'],
    setupFiles: ['tests/component/setup.ts'],
    restoreMocks: true,
    coverage: {
      provider: 'v8',
      reporter: ['text-summary', 'text'],
      include: ['src/api/**', 'src/stores/**', 'src/composables/useTauriEvent.ts', 'src/utils/progressBatch.ts', 'src/components/ui/**'],
      exclude: ['src/components/ui/UiGallery.vue', 'src/**/*.d.ts'],
    },
  },
})
