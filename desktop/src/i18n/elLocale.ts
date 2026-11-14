import { shallowRef } from 'vue'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import en from 'element-plus/es/locale/lang/en'
import type { Lang } from './index'

/** Element Plus 组件（日期选择、分页、空状态等）自带文案的语言，随界面语言切换。 */
export const elLocale = shallowRef(zhCn)

export function setElLocale(l: Lang) {
  elLocale.value = l === 'en' ? en : zhCn
}
