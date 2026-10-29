import { watch } from 'vue'
import { useAppStore } from '../stores/app'
import { setLanguage } from './index'

/** 跟随设置里的界面语言。每个窗口的根组件调用一次。 */
export function useLanguage() {
  const app = useAppStore()
  watch(() => app.settings?.language, (l) => setLanguage(l ?? 'zh'), { immediate: true })
}
