import { ref } from 'vue'
import type { HealthResult } from '../types'

/** 最近一次平台体检的结果：设置 → 解析 里点“检查”和解析页右侧的“平台健康”共用 */
export const healthResults = ref<HealthResult[] | null>(null)
export const healthAt = ref<number | null>(null)
