import { ref, watch } from 'vue'
import type { LayoutMode } from './balance'

/** 设置页卡片怎么排：自动平衡（默认）/ 按顺序 / 单列。所有页签共用，只是这台电脑上的界面偏好，记在本机。 */
const KEY = 'cc-settings-layout'

function read(): LayoutMode {
  try {
    const v = localStorage.getItem(KEY)
    return v === 'order' || v === 'single' ? v : 'balance'
  } catch {
    return 'balance'
  }
}

export const layoutMode = ref<LayoutMode>(read())
watch(layoutMode, (v) => {
  try {
    localStorage.setItem(KEY, v)
  } catch {
    /* 记不住也不影响 */
  }
})
