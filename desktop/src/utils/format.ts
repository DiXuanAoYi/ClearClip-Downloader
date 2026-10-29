export function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—'
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`
}

export function formatSpeed(bps: number): string {
  return `${formatBytes(bps)}/s`
}

export function formatDuration(ms: number | null | undefined): string {
  if (!ms) return ''
  const total = Math.round(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

export function formatEta(received: number, total: number | null, speed: number): string {
  if (!total || speed <= 0 || received >= total) return ''
  const secs = Math.ceil((total - received) / speed)
  if (secs < 60) return `剩余 ${secs} 秒`
  if (secs < 3600) return `剩余 ${Math.ceil(secs / 60)} 分钟`
  return `剩余 ${(secs / 3600).toFixed(1)} 小时`
}

export function formatDate(unixSecs: number | null | undefined): string {
  if (!unixSecs) return ''
  const d = new Date(unixSecs * 1000)
  const pad = (x: number) => String(x).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

export function formatDateTime(unixSecs: number | null | undefined): string {
  if (!unixSecs) return ''
  const d = new Date(unixSecs * 1000)
  const pad = (x: number) => String(x).padStart(2, '0')
  return `${formatDate(unixSecs)} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** 文件路径里的文件名（正反斜杠都当作分隔符）。 */
export function fileNameOf(path: string | null | undefined): string {
  return (path ?? '').split(/[\\/]/).pop() ?? ''
}

const RESERVED = /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/i

/** 与 Rust 端 naming.rs `render_base` 相同规则的文件名预览（话题标签不进文件名、标题最多 60 字、整体最多 100 字、非法字符换成下划线）。 */
export function previewFilename(
  template: string,
  sample: { author: string; title: string; id: string; platform: string; quality?: string },
  ext = 'mp4',
): string {
  const d = new Date()
  const date = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}`
  const words = sample.title.split(/\s+/).filter(Boolean)
  const kept = words.filter((w) => !w.startsWith('#') && !w.startsWith('@')).join(' ')
  const title = Array.from(kept.trim() ? kept : sample.title.trim()).slice(0, 60).join('')
  const raw = template
    .replaceAll('{author}', sample.author)
    .replaceAll('{title}', title)
    .replaceAll('{date}', date)
    .replaceAll('{id}', sample.id)
    .replaceAll('{platform}', sample.platform)
    .replaceAll('{quality}', sample.quality ?? '')
  const sanitize = (x: string) =>
    x
      // eslint-disable-next-line no-control-regex
      .replace(/[\u0000-\u001f\u007f]/g, ' ')
      .replace(/[\\/:*?"<>|]/g, '_')
      .split(/\s+/)
      .filter(Boolean)
      .join(' ')
  let name = Array.from(sanitize(raw)).slice(0, 100).join('').replace(/^[_. -]+|[_. -]+$/g, '')
  if (!name) name = sanitize(sample.id) || 'clearclip'
  if (RESERVED.test(name.split('.')[0].trim())) name += '_'
  return `${name}.${ext}`
}

/** 相对时间：刚刚、5 分钟前、3 小时前、2 天前；超过 30 天显示日期。 */
export function formatRelative(unixSecs: number | null | undefined): string {
  if (!unixSecs) return ''
  const d = Math.floor(Date.now() / 1000) - unixSecs
  if (d < 60) return '刚刚'
  if (d < 3600) return `${Math.floor(d / 60)} 分钟前`
  if (d < 86400) return `${Math.floor(d / 3600)} 小时前`
  if (d < 30 * 86400) return `${Math.floor(d / 86400)} 天前`
  return formatDate(unixSecs)
}
