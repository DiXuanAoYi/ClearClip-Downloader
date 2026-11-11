// 轨道模板：把一套轨道设置（几条轨道、名字、类型、音量、自动避让……）存下来，新工程一键套用。素材不在模板里。
import type { EditTrack } from '../types'
import { blankTrack } from './tracks.ts'

export interface TrackTemplate {
  id: string
  name: string
  desc: string
  builtin?: boolean
  tracks: EditTrack[]
}

const t = (kind: EditTrack['kind'], no: number, patch: Partial<EditTrack>): EditTrack => ({ ...blankTrack(kind, no), ...patch })

export const BUILTIN_TEMPLATES: TrackTemplate[] = [
  {
    id: 'vlog',
    name: 'Vlog',
    desc: '主轨 + 一条画中画 + 旁白 + 配乐（自动避让）+ 字幕',
    builtin: true,
    tracks: [
      t('video', 1, { name: '画中画', role: 'pip' }),
      t('audio', 1, { name: '旁白' }),
      t('audio', 2, { name: '配乐', duck: true, volume: 0.6 }),
      t('text', 1, { name: '字幕' }),
    ],
  },
  {
    id: 'podcast',
    name: '播客 / 口播',
    desc: '人声、配乐（自动避让）、音效，各一条音频轨 + 字幕',
    builtin: true,
    tracks: [
      t('audio', 1, { name: '人声' }),
      t('audio', 2, { name: '配乐', duck: true, volume: 0.5 }),
      t('audio', 3, { name: '音效' }),
      t('text', 1, { name: '字幕' }),
    ],
  },
  {
    id: 'interview',
    name: '访谈（双画面）',
    desc: '主轨 + 两条画中画 + 两条人声 + 配乐 + 字幕',
    builtin: true,
    tracks: [
      t('video', 1, { name: '嘉宾 A', role: 'pip' }),
      t('video', 2, { name: '嘉宾 B', role: 'pip' }),
      t('audio', 1, { name: '主持人' }),
      t('audio', 2, { name: '嘉宾' }),
      t('audio', 3, { name: '配乐', duck: true, volume: 0.4 }),
      t('text', 1, { name: '字幕' }),
      t('text', 2, { name: '标题' }),
    ],
  },
  {
    id: 'color',
    name: '调色 + 标题',
    desc: '主轨 + 调整图层 + 标题',
    builtin: true,
    tracks: [t('video', 1, { name: '调色', role: 'adjust' }), t('audio', 1, { name: '配乐' }), t('text', 1, { name: '标题' })],
  },
]

const KEY = 'clearclip.edit.tracktpl'

export function loadTemplates(): TrackTemplate[] {
  try {
    const list = JSON.parse(localStorage.getItem(KEY) ?? '[]')
    return Array.isArray(list)
      ? list
          .filter((x): x is TrackTemplate => !!x && typeof x.id === 'string' && typeof x.name === 'string' && Array.isArray(x.tracks))
          .map((x) => ({ ...x, desc: x.desc ?? '', builtin: false, tracks: x.tracks.map((q) => ({ ...blankTrack(q.kind, q.no), ...q })) }))
      : []
  } catch {
    return []
  }
}

export function saveTemplates(list: TrackTemplate[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(list.filter((x) => !x.builtin)))
  } catch {
    /* 存不下就只在这一次有效 */
  }
}

/** 把当前工程的轨道设置做成模板：只留有名字、类型、开关、音量等设置过的，加上有素材的轨道；锁定、独奏、隐藏这类临时状态不存。 */
export function templateFrom(name: string, tracks: EditTrack[]): TrackTemplate {
  const keep = tracks.filter((x) => !(x.kind === 'video' && x.no === 0)).map((x) => ({ ...x, solo: false, lock: false, hidden: false }))
  return { id: `u${Date.now().toString(36)}`, name: name.trim() || '我的模板', desc: `${keep.length} 条轨道`, tracks: keep }
}
