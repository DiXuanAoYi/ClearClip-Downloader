// 功能注册表：每个功能的名称、默认开关、是否需要在网页里常驻运行。不依赖浏览器接口的部分可以单元测试。

const FEATURES = [
  // group：page = 页面工具，video = 播放增强，link = 与清影联动。tag：常驻 = 在打开的每个网页里运行
  { id: 'floatButton', group: 'page', name: '视频悬浮下载按钮', desc: '鼠标移到视频上显示；可选清晰度', tag: '常驻', def: false },
  { id: 'hoverZoom', group: 'page', name: '图片悬停放大 / 画廊', desc: '按“取原图”规则找大图；规则可以自己加', tag: '常驻', def: false },
  { id: 'unlock', group: 'page', name: '解除复制 / 右键限制', desc: '只对你开启过的网站生效', tag: '常驻 · 主世界', def: false },
  { id: 'onDemand', group: 'page', name: '页面媒体总览 / 批量选取 / 截图 / 摘录', desc: '点了才注入，不常驻', tag: '按需', def: true },
  { id: 'speed', group: 'video', name: '倍速控制与快捷键', desc: '0.1×–16×；网站把倍速改回去时自动恢复', tag: '常驻', def: true },
  { id: 'speedMemory', group: 'video', name: '记住每个网站的倍速', desc: '新视频沿用上次的倍速', tag: '同上', def: true },
  { id: 'seek', group: 'video', name: '快进快退 ±10 秒', desc: '', tag: '同上', def: true },
  { id: 'videoTools', group: 'video', name: '截图、画中画、A–B 循环、逐帧', desc: '', tag: '同上', def: false },
  { id: 'videoAdvanced', group: 'video', name: '跳过片头、音量增益、画面调整', desc: '音量增益遇到跨域视频可能没有声音，会自动撤销', tag: '同上', def: false },
  { id: 'loginAsk', group: 'link', name: '“需要登录”时询问是否同步 Cookie', desc: '确认后自动重试失败的任务', tag: '无页面注入', def: true },
  { id: 'progress', group: 'link', name: '图标角标 + 下载标签 + 完成通知', desc: '', tag: '无页面注入', def: true },
  { id: 'cleanLink', group: 'link', name: '发送前自动去掉追踪参数', desc: '', tag: '无页面注入', def: true },
  { id: 'readLater', group: 'link', name: '稍后看', desc: '', tag: '无页面注入', def: true },
]

const FEATURE_GROUPS = { page: '页面工具', video: '播放增强', link: '与清影联动' }

/** 把存储里的原始值整理成完整的开关表：缺的用默认值，不认识的丢掉。 */
function normalizeFeatures(raw) {
  const r = raw && typeof raw === 'object' ? raw : {}
  const out = {}
  for (const f of FEATURES) out[f.id] = typeof r[f.id] === 'boolean' ? r[f.id] : f.def
  return out
}

/** 常驻内容脚本：key 是脚本编号，need 判断当前设置下是否要注册。文件在内容脚本里按顺序加载。 */
const RESIDENT_SCRIPTS = [
  { id: 'cc-float', js: ['content/ui.js', 'content/float-button.js'], need: (f) => f.floatButton },
  { id: 'cc-zoom', js: ['content/ui.js', 'content/original.js', 'content/hover-zoom.js'], need: (f) => f.hoverZoom },
  { id: 'cc-video', js: ['content/ui.js', 'content/video-control.js'], need: (f) => f.speed || f.speedMemory || f.seek || f.videoTools || f.videoAdvanced },
  { id: 'cc-unlock', js: ['content/unlock.js'], need: (f) => f.unlock },
]

/** 当前设置下需要注册的常驻脚本编号。 */
function neededScripts(features) {
  const f = normalizeFeatures(features)
  return RESIDENT_SCRIPTS.filter((s) => s.need(f)).map((s) => s.id)
}

const DEFAULT_SITE_RULES = {
  // 这些网站里不要显示对应功能（key：功能编号 floatButton / video / hoverZoom）
  off: { floatButton: [], video: [], hoverZoom: [] },
  // 解除限制只对这些网站生效；unlockAll：所有网站
  unlockOn: [],
  unlockAll: false,
  // 取原图的自定义规则：每行 “匹配正则 => 替换”
  zoomRules: '',
  // 追踪参数的自定义名单（每行一个）
  extraParams: '',
}

function normalizeSiteRules(raw) {
  const r = raw && typeof raw === 'object' ? raw : {}
  const hosts = (v) => [...new Set((Array.isArray(v) ? v : []).map((x) => String(x || '').trim().toLowerCase()).filter(Boolean))]
  const off = r.off && typeof r.off === 'object' ? r.off : {}
  return {
    off: { floatButton: hosts(off.floatButton), video: hosts(off.video), hoverZoom: hosts(off.hoverZoom) },
    unlockOn: hosts(r.unlockOn),
    unlockAll: r.unlockAll === true,
    zoomRules: typeof r.zoomRules === 'string' ? r.zoomRules : '',
    extraParams: typeof r.extraParams === 'string' ? r.extraParams : '',
  }
}

/** 主机名是否在名单里（名单项本身或它的子域名）。 */
function hostListed(list, host) {
  const h = String(host || '').toLowerCase()
  return (list || []).some((s) => h === s || h.endsWith('.' + s))
}

async function getFeatures() {
  const { features } = await api.storage.local.get('features')
  return normalizeFeatures(features)
}

async function getSiteRules() {
  const { siteRules } = await api.storage.local.get('siteRules')
  return normalizeSiteRules(siteRules)
}
