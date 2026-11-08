import type { SubSettings } from '../types'

/** 新建订阅的默认设置（订阅页和解析页的“订阅更新”共用）。 */
export const SUB_DEFAULTS: SubSettings = {
  intervalHours: 6,
  firstRun: 'new_only',
  firstN: 5,
  maxAuto: 20,
  include: [],
  exclude: [],
  minDurationS: null,
  maxDurationS: null,
  maxAgeDays: null,
  quality: null,
  dir: '',
  template: '',
  notify: true,
  keepLatest: 0,
  keepDays: 0,
}
