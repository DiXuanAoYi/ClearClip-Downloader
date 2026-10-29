/** 各网站的登录说明：不登录能做什么、什么时候需要登录、推荐的登录方式。与 docs/login-guide.md 保持一致。 */

export type LoginMethod = 'embedded' | 'extension' | 'cookies'

export interface SiteGuide {
  /** 内置平台 ID 或域名 */
  site: string
  name: string
  /** 不登录能做什么 */
  anonymous: string
  /** 什么时候需要登录 */
  needLogin: string
  /** 推荐方式，按顺序 */
  methods: LoginMethod[]
  /** 内置登录窗口被网站拦截（只能用扩展或 cookies.txt） */
  embeddedBlocked?: boolean
  /** 登录窗口里的提示 */
  loginTip?: string
  /** 清影能否自动识别登录完成 */
  autoDetect?: boolean
}

export const METHOD_NAME: Record<LoginMethod, string> = {
  embedded: '内置登录窗口',
  extension: '浏览器扩展同步',
  cookies: '导入 cookies.txt',
}

export const SITE_GUIDES: SiteGuide[] = [
  {
    site: 'douyin',
    name: '抖音',
    anonymous: '大多数作品可以直接解析下载。',
    needLogin: '页面拿不到作品信息、被限流，或抖音直播接口没有数据时。',
    methods: ['embedded', 'extension'],
    loginTip: '推荐用抖音 App 扫码登录。',
    autoDetect: true,
  },
  {
    site: 'kuaishou',
    name: '快手',
    anonymous: '程序会用随机设备标识匿名访问，大多数作品可以解析。',
    needLogin: '提示“触发了风控”或快手直播页没有数据时。',
    methods: ['embedded', 'extension'],
    loginTip: '推荐用快手 App 扫码登录。',
    autoDetect: true,
  },
  {
    site: 'xiaohongshu',
    name: '小红书',
    anonymous: '带 xsec_token 的完整分享链接可以直接解析。',
    needLogin: '短链接或缺少 xsec_token 的链接、仅登录可见的笔记。',
    methods: ['embedded', 'extension'],
    loginTip: '推荐用小红书 App 扫码登录，登录后点“保存登录状态”。',
  },
  {
    site: 'bilibili',
    name: 'B站',
    anonymous: '最高 720P。',
    needLogin: '1080P 需要登录；4K、HDR、杜比视界需要大会员。',
    methods: ['embedded', 'extension'],
    loginTip: '推荐用哔哩哔哩 App 扫码登录。',
    autoDetect: true,
  },
  {
    site: 'weibo',
    name: '微博',
    anonymous: '公开微博可以直接解析。',
    needLogin: '部分微博、仅粉丝可见的内容。',
    methods: ['embedded', 'extension'],
    loginTip: '登录的是微博移动版，登录后点“保存登录状态”。',
  },
  {
    site: 'pixiv',
    name: 'Pixiv',
    anonymous: '普通作品可以下载原图。',
    needLogin: 'R-18 作品，或没有返回原图地址时。',
    methods: ['embedded', 'extension'],
    loginTip: '登录后点“保存登录状态”。',
  },
  {
    site: 'youtube.com',
    name: 'YouTube',
    anonymous: '大多数视频可以直接下载。',
    needLogin: '年龄限制、会员专享、私享视频，或提示“请确认你不是机器人”时。',
    methods: ['extension', 'cookies'],
    embeddedBlocked: true,
  },
  {
    site: 'instagram.com',
    name: 'Instagram',
    anonymous: '少量公开内容可以下载。',
    needLogin: '大多数帖子、Reels 和快拍。',
    methods: ['embedded', 'extension'],
    loginTip: '可能需要邮箱或短信验证。',
    autoDetect: true,
  },
  {
    site: 'x.com',
    name: 'X（Twitter）',
    anonymous: '部分公开推文可以下载。',
    needLogin: '敏感内容、受保护账号，以及部分需要登录才能查看的推文。',
    methods: ['embedded', 'extension'],
    autoDetect: true,
  },
  {
    site: 'facebook.com',
    name: 'Facebook',
    anonymous: '公开视频可以下载。',
    needLogin: '非公开、群组或仅好友可见的内容。',
    methods: ['embedded', 'extension'],
    autoDetect: true,
  },
  {
    site: 'tiktok.com',
    name: 'TikTok',
    anonymous: '大多数视频可以直接下载。',
    needLogin: '年龄限制或私密视频。',
    methods: ['embedded', 'extension'],
    autoDetect: true,
  },
  {
    site: 'pornhub.com',
    name: 'Pornhub',
    anonymous: '大多数视频可以下载。',
    needLogin: '高清或会员内容。',
    methods: ['extension', 'cookies', 'embedded'],
  },
]

const GENERIC: Omit<SiteGuide, 'site' | 'name'> = {
  anonymous: '公开内容一般不需要登录。',
  needLogin: '出现“需要登录”“年龄限制”“会员专享”等提示时。',
  methods: ['extension', 'embedded', 'cookies'],
}

/** 谷歌系网站会拦截内嵌浏览器登录。 */
const GOOGLE = ['youtube.com', 'google.com']

export function guideFor(site: string): SiteGuide {
  const s = site.replace(/^www\./, '')
  const found = SITE_GUIDES.find((g) => g.site === s || s.endsWith(`.${g.site}`))
  if (found) return found
  return { site: s, name: s, ...GENERIC, embeddedBlocked: GOOGLE.some((g) => s === g || s.endsWith(`.${g}`)) }
}

export function siteName(site: string, providers: { id: string; name: string }[] = []): string {
  return providers.find((p) => p.id === site)?.name ?? guideFor(site).name
}
