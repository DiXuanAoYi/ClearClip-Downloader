from lib import *
S = settings_page

def s_download():
    c=[card('保存','位置',row('默认保存位置','手动下载、识别后自动下载、手机发送、订阅都按这个命名',field('D:\\清影下载','','','保存位置',220)+btn('选择','sm'))+row('空间不足时暂停下载并提示','合并音视频时需要约两倍的临时空间',sw(True,'空间'))+row('按平台建子文件夹','D:\\清影下载\\B站\\…',sw(True,'子文件夹'))+row('文件名模板','示例：山野厨房_秋天第一锅板栗焖鸡_20261112',field('{author}_{title}_{date}','','','模板',220))),
       card('队列与断点续传','并发',row('同时下载任务数','使用登录账号的网站自动减半，降低被限制的风险',stepper(3))+row('每个任务分段数','服务器支持时多线程并行下载，1 表示不分段',stepper(8))+row('全局限速（MB/s）','0 表示不限速',stepper(0))+row('启动时自动继续','关闭时恢复为“已暂停”',sw(True,'继续'))+row('失败自动重试','之后每次等待时间翻倍',stepper(3))+row('保留未完成的临时文件','保留后重新加入同一资源可接着下载',sw(True,'保留'))),
       card('批量下载','节奏',row('先逐条检查再下载','标出重复、已下载过、需要登录的',sw(True,'检查'))+row('用批次名当文件夹名','',sw(False,'批次'))+row('解析间隔','避免连续请求被网站限制',chip('随机 0.4 – 1.2 秒','cyan'))+row('展开列表时预解析','首屏 10 条，预读 20 条，其余滚动加载',sw(True,'预解析'))+row('重复文件','',seg(['跳过','改名','覆盖'],0,'重复'))),
       card('限速计划与省流量','24 小时','<div class="tl" style="height:44px;position:relative;border-radius:8px;background:var(--inset);border:1px solid var(--line);margin:4px 0 6px"><i style="position:absolute;top:0;bottom:0;left:37.5%;width:37.5%;background:var(--amber-soft);border-left:1px solid var(--amber);border-right:1px solid var(--amber)"></i><i style="position:absolute;top:0;bottom:0;left:0;width:25%;background:var(--cyan-soft);border-left:1px solid var(--cyan);border-right:1px solid var(--cyan)"></i></div><div class="mono sub-c" style="display:flex;justify-content:space-between"><span>0</span><span>6</span><span>12</span><span>18</span><span>24</span></div>'+row('启用限速计划','多个时段重叠时取最先匹配的',sw(True,'计划'))+row('按流量计费网络时省流量','并发降为 1、画质不超过 720p、不自动下载订阅更新',sw(False,'省流量'))),
       card('整理','归档',row('写入元数据','标题、作者、原链接、发布时间',sw(True,'元数据'))+row('生成 NFO 与封面图','Jellyfin / Kodi / Plex 能识别',sw(False,'NFO'))+row('合并容器','MKV 兼容的编码更多，MP4 播放兼容性更好',seg(['MP4','MKV'],0,'容器'))+row('重复内容检测','',sw(True,'重复'))),
       card('完成后','动作',row('检查下载文件','用 ffmpeg 检查；损坏时自动清理并重下一次',sw(True,'检查'))+row('完成后运行命令','高级功能，只运行你自己填写的命令',sw(False,'命令'))+'<div class="note" style="margin-top:6px">变量：CLEARCLIP_FILE、CLEARCLIP_TITLE、CLEARCLIP_AUTHOR、CLEARCLIP_URL、CLEARCLIP_PLATFORM。</div>'),
       card('剪贴板','监听',row('复制链接自动下载','按默认选项直接加入队列，不弹窗',sw(False,'剪贴板'))+row('只识别内置平台和列出的网站','避免复制普通网址时频繁弹窗',sw(True,'限定'))+row('快捷键','格式如 CommandOrControl+Shift+D',field('CommandOrControl+Shift+D','','','快捷键',200)))]
    return S('Settings.dc.html','download','下载',c)

def s_parse():
    c=[card('解析方式','',row('批量解析间隔','避免请求过快被风控',sel(['随机 0.4–1.2 秒','0.8–2 秒'],0,'间隔',150))+row('自建解析接口','只用于抖音、快手；留空则不使用',field('','https://你的接口地址','','接口',200))+row('其他网站','支持 YouTube、Twitter/X 等上千个网站；内置解析失效时也用它重试',sw(True,'yt-dlp'))+row('找网页里的视频地址','yt-dlp 也不支持时，尝试找 mp4 / m3u8',sw(True,'嗅探'))+row('微信公众号文章视频','默认关闭；请只下载你有权保存的内容',sw(False,'微信'))),
       card('清晰度与格式','',row('默认清晰度','剪贴板自动下载和批量下载也按这里选择',sel(['最高','1080P','720P'],1,'清晰度',110))+row('体积上限（MB）','超过时改选不超过的最高清晰度；0 表示不限制',stepper(0))+row('失败降一档重试','最多 3 次',sw(True,'降档'))+row('偏好编码','AV1、H.265 体积更小，但老设备可能播不了',seg(['AVC','HEVC','AV1'],0,'编码'))+row('系列命名模板','{series}/{season}/{episode} 与 {title}',field('{series}/S{season}E{episode}','','','模板',200))),
       card('字幕与弹幕','',row('偏好语言','用逗号分隔，如 zh-Hans, en',field('zh-Hans, en','','','语言',160))+row('没有人工字幕时用自动字幕','质量不如人工字幕',sw(True,'自动字幕'))+row('字幕格式','SRT 通用',seg(['SRT','VTT','ASS'],0,'格式'))+row('弹幕格式','播放器可直接显示滚动弹幕；关闭则保存 XML 原文',seg(['ASS','XML'],0,'弹幕'))+row('弹幕字号 / 留白','以 1080 高度为准',stepper(40)+stepper('10%'))),
       card('下载方式','',row('收到的链接','手机、扩展发来的与解析失败的链接记录在“媒体库 → 收到的链接”',sw(True,'记录'))+row('保留天数','0 表示不按时间清理',stepper(30))+row('最多记录条数','0 表示不限',stepper(500))),
       card('解析规则与站点','',row('规则远程下发','网站改版后自动获取新规则',sw(True,'远程'))+row('站点规则','每行一个域名，同时匹配子域名',btn('编辑','sm','edit'))+row('解析样本','用于排查解析失败，平时不需要开启',sw(False,'样本'))),
       card('平台健康检查','',f'<div class="table">{tr(["90px","minmax(0,1fr)","70px"],["B站","解析正常","0.8s"])}{tr(["90px","minmax(0,1fr)","70px"],["YouTube","解析正常","1.2s"])}{tr(["90px","minmax(0,1fr)","70px"],["抖音","需要登录","—"])}{tr(["90px","minmax(0,1fr)","70px"],["快手","解析正常","1.0s"])}</div><div style="margin-top:10px">{btn("立即检查","sm","refresh")}</div>')]
    return S('','parse','解析',c)

def s_p2p():
    c=[card('BT / 磁力','aria2',row('启用 BT 与磁力下载','由内置 aria2 提供',sw(True,'BT'))+row('监听端口','',field('6881','','','端口',90))+row('做种上限','下载完成后继续做种的分享率',field('1.0','','','分享率',90))+row('DHT / PEX / 本地发现','',sw(True,'DHT'))),
       card('ed2k','',row('启用 ed2k','需要 aria2 组件',sw(False,'ed2k'))+row('Tracker 列表','自动更新',btn('更新','sm','refresh'))),
       card('下载位置与队列','',row('P2P 下载目录','',field('D:\\清影下载\\BT','','','目录',200))+row('同时任务数','',stepper(3))+row('上传限速（MB/s）','0 表示不限速',stepper(1))),
       card('文件选择','',row('下载前选择文件','种子里可以只勾选需要的文件',sw(True,'选择'))+row('跳过小于','避免下载广告和说明文件',field('1 MB','','','小文件',90)))]
    return S('','p2p','BT 与 ed2k',c)

def s_network():
    c=[card('代理','',row('代理模式','',seg(['不使用','系统代理','手动'],1,'代理'))+row('代理地址','HTTP / SOCKS5',field('127.0.0.1:7890','','','地址',180))+row('哪些请求走代理','',sel(['全部','仅解析','仅下载'],0,'范围',110))+row('检测代理','',btn('测试连接','sm'))),
       card('请求','',row('User-Agent','',sel(['自动','Chrome','自定义'],0,'UA',110))+row('请求超时（秒）','',stepper(30))+row('DNS','',sel(['系统','DoH · Cloudflare','自定义'],0,'DNS',140))+row('强制 IPv4','',sw(False,'v4'))),
       card('本机服务','',row('开启本机 HTTP 服务','给扩展、手机和命令行用',sw(True,'服务'))+row('端口','',field('4388','','','端口',90))+row('访问令牌','只在本机使用',btn('重新生成','sm','refresh'))+row('允许局域网访问','手机互传需要',sw(True,'局域网'))),
       card('连通性','',f'<div class="list"><div class="item">{dot()}<span class="grow">解析服务器 · 可达</span><span class="mono sub-c">120 ms</span></div><div class="item">{dot("amber")}<span class="grow">YouTube · 需代理</span><span class="mono sub-c">640 ms</span></div></div>')]
    return S('','network','网络',c)

def s_accounts():
    def acc(n,st,c=''): return f'<div class="item"><div style="width:36px;height:36px;border-radius:50%;flex:none" class="thumb t1"></div><div class="grow"><div class="ttl">{n}</div><div class="meta"><span>{st}</span></div></div>{btn("退出登录" if c else "登录","sm"+(" ghost" if c else ""))}</div>'
    c=[card('账号登录','',f'<div class="list">{acc("B站 · 光影漫游者","已登录 · 大会员","y")}{acc("YouTube","已登录","y")}{acc("抖音","未登录")}{acc("小红书","未登录")}{acc("微博","已登录","y")}</div>'),
       card('Cookie','',row('Cookie 来源','',seg(['内置登录','浏览器导入','文件'],0,'来源'))+row('从浏览器导入','Chrome / Edge / Firefox',btn('选择浏览器','sm'))+row('Cookie 文件','Netscape 格式',btn('导入','sm','folder'))+'<p class="note">密钥与 Cookie 只存在本机设置里，不会上传。</p>'),
       card('登录状态','',row('自动刷新登录','过期前尝试续期',sw(True,'刷新'))+row('登录失效时提醒','',sw(True,'提醒')))]
    return S('','accounts','账号与 Cookie',c)

def s_phone():
    c=[card('手机互传','局域网',row('开启手机互传','同一 Wi-Fi 下扫码配对',sw(True,'互传'))+row('配对验证码','6 位，每次配对重新生成',chip('482 913','cyan'))+row('新设备配对需要确认','电脑上点“允许”后才能互传',sw(True,'确认'))+row('已配对设备','Pixel 8、iPad',btn('管理','sm'))),
       card('浏览器扩展','',row('扩展状态','Chrome 1.24.0 · 已连接',chip(dot()+'已连接','cyan'))+row('用扩展下载时','',seg(['加入队列','先预览'],0,'扩展'))+row('扩展悬浮窗','视频胶囊 + 划词翻译卡片',sw(True,'悬浮窗'))+row('安装扩展','Chrome / Edge / Firefox',btn('打开安装页','sm'))),
       card('手机端','',row('手机网页主题','',seg(['跟随系统','深色','浅色'],0,'主题'))+row('接收文件保存到','',field('D:\\清影下载\\手机','','','位置',180))+row('缩略图','在列表里显示预览',sw(True,'缩略图')))]
    return S('','phone','手机与浏览器扩展',c)

def s_auto():
    c=[card('订阅与定时','',row('开启订阅','在托盘运行，用于定期检查',sw(True,'订阅'))+row('默认检查间隔','',sel(['1 小时','6 小时','12 小时'],1,'间隔',110))+row('开机自动启动','',sw(True,'开机'))),
       card('命令行与 HTTP API','',row('命令行 clearclip','解析、下载、健康检查',btn('复制路径','sm'))+row('HTTP API','给脚本和扩展调用',sw(True,'API'))+row('MCP 服务','让 AI 助手调用清影',sw(False,'MCP'))),
       card('完成后自动处理','',row('下载后转码','',sel(['不处理','压缩到 720P','提取音频'],0,'转码',140))+row('下载后生成字幕','',sw(False,'字幕'))+row('下载后翻译字幕','',sw(False,'翻译'))),
       card('通知','',row('完成通知','',seg(['全部','仅失败','关闭'],0,'通知'))+row('声音','',sw(False,'声音')))]
    return S('','auto','自动化',c)

def s_ai():
    def svc(n,m,ok=True): return f'<div class="item"><div class="grow"><div class="ttl">{n}</div><div class="meta"><span class="mono">{m}</span></div></div>{chip(dot()+("可用" if ok else "未检测"),"cyan" if ok else "")}{btn("检测模型","sm")}{btn("","sm ghost","more")}</div>'
    c=[card('服务商池','AiPool',f'<div class="list">{svc("OpenAI 兼容 · 自建","gpt-4o-mini, gpt-4o")}{svc("DeepSeek","deepseek-chat")}{svc("腾讯翻译 / 阿里 / 火山","机器翻译",False)}</div><div style="margin-top:10px">{btn("添加服务","sm","plus")}</div>'),
       card('默认模型','',row('文字模型','摘要、纠错、翻译',sel(['gpt-4o-mini','deepseek-chat'],0,'文字',160))+row('视觉模型','屏幕翻译、漫画模式',sel(['gpt-4o','未选择'],0,'视觉',140))+row('目标语言','翻译与摘要的默认语言',field('简体中文','','','语言',140))),
       card('语音转文字','',row('引擎','',sel(['SenseVoice 本机','Whisper 本机','云端'],0,'引擎',150))+row('断句长度','规则断句，字幕按句长拆分',stepper(28))+row('流式转写','边下载边出字幕',sw(True,'流式'))),
       card('术语与专家','',row('术语库','按领域分层，可导入导出',btn('打开','sm','book'))+row('专家','为不同场景预设提示词',btn('管理','sm'))+row('翻译缓存','相同句子不重复请求',sw(True,'缓存'))),
       card('用量与隐私','',row('用量统计','本月 1.2 万次请求',btn('查看','sm'))+row('省流量时不调用 AI','',sw(True,'省流量')))]
    return S('','ai','AI',c)

def s_writing():
    c=[card('人设','14 个',row('当前人设','写作助手的语气和领域',sel(['通用助手','技术写作','文案','学术','小红书'],0,'人设',150))+row('自定义人设','写自己的系统提示',btn('新建','sm','plus'))),
       card('记忆','',row('记忆开关','记住你的术语和偏好；只存在本机',sw(True,'记忆'))+row('清空记忆','',btn('清空','sm danger'))),
       card('写作窗口','',row('快捷键','',field('Ctrl+Alt+W','','','快捷键',140))+row('选中文字后出现按钮','润色 / 翻译 / 摘要',sw(True,'按钮'))+row('输出','',seg(['替换原文','插到下方','复制'],0,'输出'))),
       card('思维导图','',row('生成第二张图','按章节再生成一张',sw(True,'第二图'))+row('导出','',sel(['PNG','SVG','Markdown'],0,'导出',110)))]
    return S('','writing','写作',c)

def s_security():
    c=[card('应用锁','',row('启动时需要密码','',sw(True,'密码'))+row('自动锁定','无操作后',sel(['5 分钟','10 分钟','30 分钟'],1,'锁定',110))+row('修改密码','',btn('修改','sm'))),
       card('加密保险箱','',row('保险箱','文件加密保存，没有密码无法找回',chip('已创建 · 已锁定'))+row('解锁','',btn('解锁','sm','lock'))+row('创建新的保险箱','',btn('创建','sm'))),
       card('隐私','',row('不保存解析历史','',sw(False,'历史'))+row('日志脱敏','不含密钥、Cookie 与完整路径',sw(True,'脱敏'))+row('清除数据','历史、缓存、Cookie',btn('清除…','sm danger'))),
       card('权限','',row('允许局域网设备访问','',sw(True,'局域网'))+row('允许扩展访问','',sw(True,'扩展'))+row('下载前确认来源','来自手机或扩展的任务',sw(False,'确认')))]
    return S('','security','安全与隐私',c)

def s_components():
    def comp(n,v,ok=True,sz=''): return f'<div class="item"><div class="grow"><div class="ttl">{n}</div><div class="meta"><span class="mono">{v}</span><span>{sz}</span></div></div>{chip("已安装" if ok else "未安装","cyan" if ok else "")}{btn("更新" if ok else "安装","sm")}</div>'
    c=[card('核心组件','',f'<div class="list">{comp("ffmpeg","7.1",True,"98 MB")}{comp("yt-dlp","2026.11.02",True,"18 MB")}{comp("aria2","1.37",True,"6 MB")}</div>'),
       card('可选组件','',f'<div class="list">{comp("PDFium（PDF 排版翻译）","6842",True,"22 MB")}{comp("PaddleOCR 增强识字","1.0",True,"15 MB")}{comp("SenseVoice 语音模型","small",True,"230 MB")}{comp("Whisper.cpp","—",False,"")}</div>'),
       card('更新','',row('自动检查更新','启动时检查程序与组件',sw(True,'更新'))+row('更新通道','',seg(['稳定版','预览版'],0,'通道'))+row('当前版本','清影 1.15.1 · 扩展 1.24.0',btn('检查更新','sm','refresh'))),
       card('存放位置','',row('组件目录','',field('C:\\Users\\…\\ClearClip\\components','','','目录',220))+row('清理旧版本','释放 420 MB',btn('清理','sm')))]
    return S('','components','组件',c)

def s_diag():
    c=[card('平台健康','',f'<div class="list"><div class="item">{dot()}<span class="grow">B站 · 解析正常</span><span class="mono sub-c">0.8s</span></div><div class="item">{dot("amber")}<span class="grow">抖音 · 需要登录</span></div></div><div style="margin-top:10px">{btn("立即检查","sm","refresh")}{btn("复制报告","sm")}</div>'),
       card('日志','',row('日志级别','',sel(['信息','调试'],0,'级别',100))+row('打开日志目录','',btn('打开','sm','folder'))+row('导出诊断包','脱敏，可直接发给维护者',btn('导出','sm','dl'))),
       card('WebView 与系统','',f'<dl class="kv2"><dt>系统</dt><dd>Windows 11 23H2</dd><dt>WebView2</dt><dd>125.0.2535.51</dd><dt>程序</dt><dd>1.15.1</dd><dt>数据目录</dt><dd class="mono">D:\\清影\\data</dd></dl>'),
       card('故障恢复','',row('重建数据库索引','文件有变化却没显示时',btn('重建','sm'))+row('重置设置','保留下载文件',btn('重置…','sm danger')))]
    return S('','diag','诊断',c)

def s_general():
    c=[card('外观与行为','',row('主题','',seg(['跟随系统','深色','浅色'],1,'主题'))+row('界面语言','English 界面由机器翻译整理',sel(['简体中文','English'],0,'语言',120))+row('设置页排布','自动平衡会让各列一样高；按顺序严格按先后放；单列最简单',seg(['自动平衡','按顺序','单列'],0,'排布'))+row('悬浮窗','把链接拖进去就开始下载；双击回到主窗口',sw(True,'悬浮窗'))+row('关闭窗口时','在托盘运行，用于订阅的定期检查',seg(['最小化到托盘','退出'],0,'关闭'))+row('便携模式','设置、数据库和日志保存在程序目录下',sw(False,'便携'))),
       card('媒体库','',row('删除时移到回收站','隐藏文件夹，可以还原',sw(True,'回收站'))+row('自动清空回收站','超过天数的永久删除，0 表示不清空',stepper(30))+row('读取视频时长与封面','用 ffmpeg；没有封面的视频会截一张图',sw(True,'封面'))+row('整理文件夹模板','变量：{platform} {author} {year} {month}',field('{platform}/{author}/{year}','','','模板',200))),
       card('快捷键','',row('呼出命令面板','',f'<span class="kbd">Ctrl</span><span class="kbd">K</span>')+row('截图翻译','',f'<span class="kbd">Ctrl</span><span class="kbd">Alt</span><span class="kbd">T</span>')+row('剪贴板下载','',f'<span class="kbd">Ctrl</span><span class="kbd">Shift</span><span class="kbd">D</span>')),
       card('关于','',f'<dl class="kv2"><dt>版本</dt><dd>清影 1.15.1</dd><dt>浏览器扩展</dt><dd>1.24.0</dd><dt>许可</dt><dd>开源</dd></dl><div style="margin-top:10px">{btn("检查更新","sm","refresh")}{btn("更新记录","sm")}</div>')]
    return S('','general','通用',c)
