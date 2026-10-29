/**
 * 界面语言。
 *
 * 做法：界面源码里写的是中文，英文模式下由这里在 DOM 上把中文文字换成 `en.ts` 里的译文，
 * 切回中文时再换回来。好处是页面和组件不用改写成“键 → 文字”的形式，新增的中文文字
 * 只要往 `en.ts` 里补一行；缺少译文的地方保持中文，不会出错。
 *
 * - 文本节点和 placeholder / title / aria-label / alt 属性都会处理（弹出的提示、对话框也一样）。
 * - 带变量的文字用 `{0}`、`{1}` 占位，匹配时按顺序取出变量，变量里的中文也会再翻译一次。
 * - 用户自己的内容（标题、作者、文件名）只有和词条完全相同才会被翻译，一般不会。
 * - 后端返回的错误消息等不在源码里，不会被翻译。
 */

import { en } from './en'

export type Lang = 'zh' | 'en' | 'ja'

/** 目前有译文的语言（界面里的语言选项只列这些）。 */
export const LANGUAGES: { value: Lang; label: string }[] = [
  { value: 'zh', label: '中文' },
  { value: 'en', label: 'English' },
]

const DICTS: Partial<Record<Lang, Record<string, string>>> = { en }
const CJK = /[一-鿿]/
const ATTRS = ['placeholder', 'title', 'aria-label', 'alt'] as const

interface Pattern {
  re: RegExp
  tpl: string
}

let lang: Lang = 'zh'
let exact: Record<string, string> = {}
let patterns: Pattern[] = []
let observer: MutationObserver | null = null
const cache = new Map<string, string | null>()
/** 文本节点被我们改成了什么：{ 原文, 译文 } */
const textState = new WeakMap<Node, { orig: string; out: string }>()
const attrState = new WeakMap<Element, Map<string, { orig: string; out: string }>>()

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** `已选 {0} 项` → /^已选\s+(.+?)\s+项$/ */
export function skeletonToRegex(key: string): RegExp {
  const body = escapeRe(key)
    .replace(/\\\{\d+\\\}/g, '(.+?)')
    .replace(/ +/g, '\\s+')
  return new RegExp(`^${body}$`, 's')
}

function build(d: Record<string, string> | undefined) {
  exact = {}
  patterns = []
  cache.clear()
  if (!d) return
  const withVars: [string, string][] = []
  for (const [k, v] of Object.entries(d)) {
    if (/\{\d+\}/.test(k)) withVars.push([k, v])
    else exact[k] = v
  }
  // 长的、更具体的条目优先
  withVars.sort((a, b) => b[0].length - a[0].length)
  patterns = withVars.map(([k, v]) => ({ re: skeletonToRegex(k), tpl: v }))
}

const DELIM = /(\s*[·/|、，,；;：:]\s*|\s+-\s+)/
const DELIM_MAP: Record<string, string> = { '、': ', ', '，': ', ', '；': '; ', '：': ': ' }

/** 先按词条、再按带变量的词条匹配；`deep` 为真时允许把整段按分隔符拆开逐段翻译（用于变量里的中文）。 */
function lookup(core: string, deep: boolean): string | null {
  const direct = exact[core]
  if (direct !== undefined) return direct
  for (const p of patterns) {
    const mm = p.re.exec(core)
    if (mm) return p.tpl.replace(/\{(\d+)\}/g, (_, i: string) => {
      const cap = mm[Number(i) + 1] ?? ''
      return translateCore(cap) ?? cap
    })
  }
  if (!deep) return null
  // 没有整句译文：按 “ · ”“、”“，” 等分隔符拆开，能翻译的段落翻译（例如 “127.0.0.1 · 视频”）
  const parts = core.split(DELIM)
  if (parts.length < 3) return null
  let changed = false
  const out = parts.map((p, i) => {
    if (i % 2 === 1) return DELIM_MAP[p.trim()] ?? p
    const t = lookup(p.trim(), false)
    if (t === null) return p
    changed = true
    return t
  })
  return changed ? out.join('') : null
}

function translateCore(core: string): string | null {
  if (!CJK.test(core)) return null
  const norm = core.replace(/\s+/g, ' ').trim()
  const hit = cache.get(norm)
  if (hit !== undefined) return hit
  const out = lookup(norm, true)
  if (cache.size > 6000) cache.clear()
  cache.set(norm, out)
  return out
}

/** 翻译一段文字；没有对应译文（或不含中文）时返回 null。保留首尾空白。 */
export function translate(s: string): string | null {
  if (!CJK.test(s)) return null
  const m = /^(\s*)([\s\S]*?)(\s*)$/.exec(s)
  if (!m) return null
  const out = translateCore(m[2])
  return out === null ? null : m[1] + out + m[3]
}

const SKIP_TAGS = new Set(['SCRIPT', 'STYLE', 'TEXTAREA', 'NOSCRIPT'])

/** 文字内容不翻译的位置：脚本、输入框里用户写的内容、标了 data-no-i18n 的区域。`attrOnly` 时只看后者（输入框的 placeholder 要翻译）。 */
function skipped(el: Element | null, attrOnly = false): boolean {
  for (let e = el; e; e = e.parentElement) {
    if (e.hasAttribute('data-no-i18n')) return true
    if (!attrOnly && (SKIP_TAGS.has(e.tagName) || (e as HTMLElement).isContentEditable)) return true
  }
  return false
}

function applyText(node: Text) {
  if (skipped(node.parentElement)) return
  const cur = node.nodeValue ?? ''
  const st = textState.get(node)
  if (lang === 'zh') {
    if (st && cur === st.out) node.nodeValue = st.orig
    textState.delete(node)
    return
  }
  // 已经是我们写入的译文：保持；否则（Vue 刷新了文字）把当前内容当作新的原文
  const orig = st && cur === st.out ? st.orig : cur
  const out = translate(orig)
  if (out === null) {
    textState.delete(node)
    return
  }
  if (out !== cur) node.nodeValue = out
  textState.set(node, { orig, out })
}

function applyAttr(el: Element, name: string) {
  if (skipped(el, true)) return
  const cur = el.getAttribute(name)
  if (cur === null) return
  let map = attrState.get(el)
  const st = map?.get(name)
  if (lang === 'zh') {
    if (st && cur === st.out) el.setAttribute(name, st.orig)
    map?.delete(name)
    return
  }
  const orig = st && cur === st.out ? st.orig : cur
  const out = translate(orig)
  if (out === null) {
    map?.delete(name)
    return
  }
  if (out !== cur) el.setAttribute(name, out)
  if (!map) attrState.set(el, (map = new Map()))
  map.set(name, { orig, out })
}

function walk(root: Node) {
  if (root.nodeType === Node.TEXT_NODE) {
    applyText(root as Text)
    return
  }
  if (root.nodeType !== Node.ELEMENT_NODE && root.nodeType !== Node.DOCUMENT_NODE) return
  if (root.nodeType === Node.ELEMENT_NODE) for (const a of ATTRS) applyAttr(root as Element, a)
  const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT)
  for (let n = w.nextNode(); n; n = w.nextNode()) {
    if (n.nodeType === Node.TEXT_NODE) applyText(n as Text)
    else for (const a of ATTRS) applyAttr(n as Element, a)
  }
}

function onMutations(records: MutationRecord[]) {
  for (const r of records) {
    if (r.type === 'childList') r.addedNodes.forEach(walk)
    else if (r.type === 'characterData') applyText(r.target as Text)
    else if (r.type === 'attributes' && r.attributeName) applyAttr(r.target as Element, r.attributeName)
  }
}

function syncTitle() {
  const t = document.title
  if (lang === 'zh') return
  const out = translate(t)
  if (out) document.title = out
}

export function currentLang(): Lang {
  return lang
}

/** 切换界面语言。没有译文的语言按中文显示。 */
export function setLanguage(next: Lang) {
  const effective: Lang = DICTS[next] ? next : 'zh'
  if (effective === lang && (effective === 'zh') === (observer === null)) return
  lang = effective
  document.documentElement.lang = lang === 'zh' ? 'zh-CN' : lang
  build(DICTS[lang])
  // 英文 → 中文：先把改过的节点换回原文，再停止监听
  walk(document)
  if (lang === 'zh') {
    observer?.disconnect()
    observer = null
    return
  }
  syncTitle()
  if (!observer) {
    observer = new MutationObserver(onMutations)
    observer.observe(document.documentElement, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: [...ATTRS] })
  }
}

// 开发时修改这个文件要整页刷新：热更新会同时留下两份模块，各自记着自己译过的节点，切回中文时就换不回来
if (import.meta.hot) import.meta.hot.accept(() => location.reload())
