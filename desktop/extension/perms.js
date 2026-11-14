// 可选权限：Cookie、所有网站的访问权限都不在安装时索取，用到对应功能时才通过 permissions.request 申请。
// 申请必须由用户手势触发（弹窗 / 设置页里点开关或按钮），所以 request 只在 popup.js 里调用；后台只检查（contains）。
// 这是普通全局脚本（后台 / 扩展页面 / 单元测试都用 <script> / importScripts / vm 加载）。

const ccPerms = (() => {
  const apiX = () => globalThis.browser ?? globalThis.chrome

  /** 主机名是否能放进匹配模式（防止把奇怪的字符串变成通配）。 */
  const validHost = (h) => /^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$/.test(String(h || ''))

  /** 某个网站（含子域名）的访问范围。 */
  function siteOrigins(site) {
    const s = String(site || '').toLowerCase()
    return validHost(s) ? [`*://${s}/*`, `*://*.${s}/*`] : []
  }

  /** 同步某个网站的登录 Cookie 要的权限：cookies + 该网站的访问权限。 */
  const cookieRequest = (site) => ({ permissions: ['cookies'], origins: siteOrigins(site) })

  /** 常驻脚本 / 视频嗅探 / Alt 点击放行要的权限：所有网页的访问权限。 */
  const sitesRequest = () => ({ origins: ['http://*/*', 'https://*/*'] })

  const normalize = (req) => ({ ...(req.permissions?.length ? { permissions: req.permissions } : {}), ...(req.origins?.length ? { origins: req.origins } : {}) })

  /** 已经有这些权限吗。没有 permissions 接口（极旧的浏览器）时当作有，保持旧行为。 */
  async function has(req) {
    const r = normalize(req)
    if (!r.permissions && !r.origins) return false
    const p = apiX()?.permissions
    if (!p?.contains) return true
    try {
      return !!(await p.contains(r))
    } catch {
      return false
    }
  }

  /** 向用户申请（弹出浏览器的授权框）。只能在用户点击的处理函数里调用，而且前面不要先 await 别的。拒绝或出错返回 false。 */
  async function request(req) {
    const r = normalize(req)
    if (!r.permissions && !r.origins) return false
    const p = apiX()?.permissions
    if (!p?.request) return true
    try {
      return !!(await p.request(r))
    } catch {
      return false
    }
  }

  const hasSites = () => has(sitesRequest())
  const hasCookies = (site) => has(cookieRequest(site))

  const TEXT = {
    cookies: '需要“Cookie”权限和这个网站的访问权限才能同步登录状态：在弹窗里点“授权”，浏览器会问你一次。',
    sites: '还没有授权“访问所有网页”：视频嗅探、划词工具条、写作浮窗、播放增强、音效这类在网页里常驻的功能暂时不可用。在“设置”里点“授权访问网页”开启；也可以只用快捷键或点工具栏图标按需使用。',
  }

  /** 给“没有权限”的错误一个稳定的 code，界面据此显示引导。 */
  function permError(kind) {
    const e = new Error(TEXT[kind] || TEXT.sites)
    e.code = 'noperm'
    e.perm = kind
    return e
  }

  return { validHost, siteOrigins, cookieRequest, sitesRequest, has, request, hasSites, hasCookies, permError, TEXT }
})()
