// 按文件开头的字节和响应类型认出媒体的真实格式（总览的格式徽标、保存图片时命名都用它）。
// 规则和应用端 grab.rs 的 sniff_ext 一致。
;(() => {
  const MIME = {
    jpg: 'image/jpeg', png: 'image/png', gif: 'image/gif', webp: 'image/webp', avif: 'image/avif', heic: 'image/heic', bmp: 'image/bmp', ico: 'image/x-icon', svg: 'image/svg+xml',
    mp4: 'video/mp4', webm: 'video/webm', mov: 'video/quicktime', m4a: 'audio/mp4', mp3: 'audio/mpeg', ogg: 'audio/ogg', flac: 'audio/flac', wav: 'audio/wav', pdf: 'application/pdf', zip: 'application/zip',
  }
  const EXT_OF_MIME = Object.fromEntries(Object.entries(MIME).map(([e, m]) => [m, e]))
  EXT_OF_MIME['image/jpg'] = 'jpg'
  EXT_OF_MIME['audio/x-m4a'] = 'm4a'
  EXT_OF_MIME['audio/mp3'] = 'mp3'
  EXT_OF_MIME['video/x-matroska'] = 'webm'

  const ascii = (b, from, to) => String.fromCharCode(...b.slice(from, to))

  /** 返回扩展名（小写，不含点），认不出返回空串。 */
  function sniffExt(bytes) {
    const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes || [])
    const at = (...v) => v.every((x, i) => b[i] === x)
    if (at(0xff, 0xd8, 0xff)) return 'jpg'
    if (at(0x89, 0x50, 0x4e, 0x47)) return 'png'
    if (ascii(b, 0, 4) === 'GIF8') return 'gif'
    if (b.length >= 12 && ascii(b, 0, 4) === 'RIFF') {
      const t = ascii(b, 8, 12)
      if (t === 'WEBP') return 'webp'
      if (t === 'WAVE') return 'wav'
    }
    if (b.length >= 12 && ascii(b, 4, 8) === 'ftyp') {
      const brand = ascii(b, 8, 12)
      if (brand === 'avif' || brand === 'avis') return 'avif'
      if (['heic', 'heix', 'hevc', 'mif1', 'msf1'].includes(brand)) return 'heic'
      if (brand === 'M4A ' || brand === 'M4B ') return 'm4a'
      if (brand === 'qt  ') return 'mov'
      return 'mp4'
    }
    if (at(0x1a, 0x45, 0xdf, 0xa3)) return 'webm'
    if (ascii(b, 0, 2) === 'BM' && b.length > 14) return 'bmp'
    if (at(0, 0, 1, 0)) return 'ico'
    if (ascii(b, 0, 3) === 'ID3' || (b[0] === 0xff && (b[1] & 0xe0) === 0xe0)) return 'mp3'
    if (ascii(b, 0, 4) === 'OggS') return 'ogg'
    if (ascii(b, 0, 4) === 'fLaC') return 'flac'
    if (ascii(b, 0, 4) === '%PDF') return 'pdf'
    if (at(0x50, 0x4b, 3, 4)) return 'zip'
    const t = ascii(b, 0, Math.min(b.length, 512)).toLowerCase().replace(/^﻿/, '').trimStart()
    if (t.startsWith('<svg') || (t.startsWith('<?xml') && t.includes('<svg'))) return 'svg'
    return ''
  }

  const extOfMime = (m) => EXT_OF_MIME[String(m || '').split(';')[0].trim().toLowerCase()] || ''
  const extOfUrl = (u) => {
    try {
      const m = new URL(u).pathname.match(/\.([A-Za-z0-9]{2,5})$/)
      const e = m ? m[1].toLowerCase() : ''
      return e === 'jpeg' || e === 'jfif' ? 'jpg' : e
    } catch {
      return ''
    }
  }
  const FORMAT_LABEL = { jpg: 'JPEG', png: 'PNG', gif: 'GIF', webp: 'WebP', avif: 'AVIF', heic: 'HEIC', bmp: 'BMP', ico: 'ICO', svg: 'SVG', mp4: 'MP4', webm: 'WebM', mov: 'MOV', m4a: 'M4A', mp3: 'MP3', ogg: 'OGG', flac: 'FLAC', wav: 'WAV' }

  /** 给地址起一个带真实后缀的文件名：已经正确就保持，没有或不对就换成真实格式。 */
  function fileNameFor(url, real) {
    let base = 'file'
    try {
      const seg = decodeURIComponent(new URL(url).pathname.split('/').filter(Boolean).pop() || '')
      base = seg.replace(/\.[A-Za-z0-9]{2,5}$/, '') || new URL(url).hostname
    } catch {
      // 地址不规范：用默认名
    }
    base = base.replace(/[\\/:*?"<>|\u0000-\u001f]/g, '_').slice(0, 80) || 'file'
    return real ? `${base}.${real}` : base
  }

  globalThis.ccSniff = { sniffExt, extOfMime, extOfUrl, FORMAT_LABEL, MIME, fileNameFor }
})()
