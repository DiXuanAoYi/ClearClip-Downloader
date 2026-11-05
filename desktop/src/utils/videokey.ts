/** 和浏览器扩展、后端 videocache.rs 一致的“同一个视频”的键。 */
export function videoKey(href: string): string {
  let u: URL
  try {
    u = new URL(href)
  } catch {
    return `url:${href}`
  }
  const host = u.hostname.replace(/^www\.|^m\./, '')
  if (host === 'youtu.be') {
    const id = u.pathname.slice(1).split('/')[0]
    if (id) return `youtube:${id}`
  }
  if (host === 'youtube.com' || host === 'music.youtube.com') {
    const v = u.searchParams.get('v')
    if (v) return `youtube:${v}`
    const m = u.pathname.match(/^\/(?:shorts|embed|live|v)\/([\w-]{6,})/)
    if (m) return `youtube:${m[1]}`
  }
  if (host === 'bilibili.com') {
    const m = u.pathname.match(/\/video\/(BV\w+|av\d+)/i)
    if (m) {
      const p = Number(u.searchParams.get('p')) || 1
      return p > 1 ? `bilibili:${m[1]}:p${p}` : `bilibili:${m[1]}`
    }
  }
  u.hash = ''
  return `url:${u.href}`
}
