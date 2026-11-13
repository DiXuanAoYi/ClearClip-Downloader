from lib import *

def plain(title, h, body, css=''):
    return f'''<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>清影 · {title}</title>
<script src="./support.js"></script>
<link rel="stylesheet" href="clearclip.css">
</head>
<body>
<x-dc>
<helmet>
{FONTS}
<style>
body {{ margin: 0; }}
.cc .plainmain {{ padding: 22px 28px 32px; display: flex; flex-direction: column; gap: 18px; min-height: {h}px; }}
{css}
</style>
</helmet>
<div class="cc" data-theme="{{{{theme}}}}">
<div class="plainmain">
<header class="topbar"><h1>{title}</h1><span class="spacer"></span></header>
{body}
</div>
</div>
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{{"theme":{{"editor":"enum","default":"dark","options":["dark","light"],"section":"外观"}},"$preview":{{"width":1360,"height":{h}}}}}'>
class Component extends DCLogic {{
  renderVals() {{ return {{ theme: this.props.theme ?? 'dark' }}; }}
}}
</script>
</body>
</html>
'''

def cap(t): return f'<div class="frame-cap">{t}</div>'
def winh(title, right=''): return f'<div class="win-h"><span class="logo-s">清</span><b>{title}</b><span class="spacer"></span>{right}</div>'

# ---------------- 清影写作 ----------------
def p_write():
    w=464
    reply = f'''<div>{cap("桌面端 · 回复")}<div class="win" style="width:{w}px">{winh("清影写作", chip("固定在最前","")+btn("","sm ghost","set")+btn("","sm ghost","x"))}
<div class="win-b">
{tabs(["回复","润色","翻译","摘录"],0)}
<div class="lbl">收到的消息</div>
<div class="bubble">下周的评审会可以提前到周三下午吗？另外上次说的那份报价，今天能发我吗？</div>
<div class="lbl">前面的聊天（只用来理解，不会回复它们）</div>
{field("","粘贴前面的几条聊天","","前面的聊天")}
<div class="hint-bar cyan"><b>看懂了：</b>对方想把评审会提前到周三下午，并催今天要报价。</div>
<div class="lbl">我的意思</div>
{seg(["同意","婉拒","给个替代"],2,"意图")}
<div class="lbl">要问的（自动找出没说清的地方，可以勾掉或自己加）</div>
<div>{check("周三下午几点方便？",True)}<br>{check("报价要含税吗？",True)}</div>
<div class="toolbar">{field("","＋ 自己加一个问题","","加问题",260)}{btn("加上","sm")}</div>
<div class="lbl">怎么说</div>
{seg(["简短","适中","详细"],1,"长度")}
{sel(["场景：通用","场景：对上级","场景：对客户","场景：朋友"],0,"场景")}
<div class="toolbar">{btn("生成回复","primary","bolt")}<span class="spacer"></span><span class="kbd">Ctrl</span><span class="kbd">↵</span></div>
</div>
<div class="win-f"><span>只用你给的条件和时间，不替你编理由、不多承诺。</span></div></div></div>'''
    result = f'''<div>{cap("桌面端 · 三个版本 / 对比")}<div class="win" style="width:{w}px">{winh("清影写作", chip("1/3","cyan"))}
<div class="win-b">
<div class="verc on"><div class="vh"><b style="color:var(--amber)">版本 1</b><span>简短</span><span class="spacer"></span><span class="kbd">Alt+1</span>{btn("","sm ghost","star")}</div><div class="vt">周三下午我这边可以，三点怎么样？报价今天下班前发你。</div><div class="toolbar">{btn("复制","sm")}{btn("再来一版","sm")}{btn("微调","sm")}{btn("对比","sm")}</div></div>
<div class="verc"><div class="vh"><b>版本 2</b><span>适中</span><span class="spacer"></span><span class="kbd">Alt+2</span>{btn("","sm ghost","star")}</div><div class="vt">周三下午可以的，你看三点还是四点更方便？报价我今天下班前整理好发你，含税的话我一并标注。</div></div>
<div class="verc"><div class="vh"><b>版本 3</b><span>详细</span><span class="spacer"></span><span class="kbd">Alt+3</span>{btn("","sm ghost","star")}</div><div class="vt">评审会提前到周三下午没问题。时间上三点或四点都行，你定一个就好；报价今天下班前发你，是否含税请告诉我。</div></div>
<div class="verc"><div class="vh"><b>对比</b><span>原文 → 修改</span></div><div class="vt"><span class="diffdel">周三下午我这边可以</span><span class="diffins">周三下午可以的</span>，三点怎么样？</div></div>
<div class="toolbar">{check("写回后自动关闭",True)}<span class="spacer"></span>{btn("撤销","sm ghost")}{btn("取消","sm")}</div>
</div>
<div class="win-f"><span>快捷键 修改 · Esc 关闭 · Alt+1/2/3 切版本</span></div></div></div>'''
    polish = f'''<div>{cap("桌面端 · 润色 / 校对 / 翻译 / 摘录")}<div class="win" style="width:{w}px">{winh("清影写作")}
<div class="win-b">
{tabs(["回复","润色","翻译","摘录"],1)}
<div class="lbl">选一个人设</div>
<div class="toolbar">{chip("通用助手","acc")}{chip("技术写作")}{chip("文案")}{chip("学术")}{chip("小红书")}</div>
<div class="lbl">要处理的文字</div>
<textarea aria-label="要处理的文字">这个功能上线以后用户反馈很好但是我们还需要继续优化一下细节</textarea>
<div class="toolbar">{btn("粘贴","sm")}{btn("清空","sm")}<span class="spacer"></span>{sel(["听写：跟随设置","本机开源模型","在线接口"],0,"听写引擎",150)}</div>
{field("","额外要求（可选，比如：别太长、带点东北话）","","额外要求")}
<div class="hint-bar">没有发现需要修改的地方时，这里会直接告诉你。</div>
<div class="toolbar">{btn("润色","primary")}{btn("校对（找错别字、语病、标点）")}</div>
</div></div></div>'''
    saved = f'''<div>{cap("桌面端 · 摘录列表 / 我的收藏")}<div class="win" style="width:{w}px">{winh("清影写作")}
<div class="win-b">
{tabs(["回复","润色","翻译","摘录"],3)}
<div class="toolbar">{btn("存入摘录","sm")}{btn("复制为 Markdown","sm")}<span class="spacer"></span>{seg(["摘录列表","我的收藏"],0,"列表")}</div>
<div class="item"><div class="grow"><div style="font-size:13.5px">夜里的外滩，像一台缓慢运转的巨大钟表。</div><div class="meta"><span>光影漫游者</span><span>02:10</span></div></div>{btn("放回输入框","sm")}{btn("删除","sm danger")}</div>
<div class="item"><div class="grow"><div style="font-size:13.5px">The best way to predict the future is to invent it.</div><div class="meta"><span>Alan Kay</span></div></div>{btn("放回输入框","sm")}{btn("删除","sm danger")}</div>
<div class="sec-t">对选中的文字</div><div class="toolbar">{btn("释义","sm")}{btn("AI 搜索","sm")}</div>
</div></div></div>'''
    badge = f'''<div>{cap("写作小徽标（选中文字后出现）")}<div style="display:flex;gap:16px;align-items:center"><span class="cap-pill" style="font-size:14px;padding:8px 14px;border-color:var(--amber-line);color:var(--amber)">写</span><span class="note">选中文字后在旁边出现，点一下或按快捷键打开清影写作。</span></div></div>'''
    page_ = f'''<div>{cap("浏览器端 · 在输入框旁弹出的写作浮窗")}
<div class="fakepage" style="min-height:560px"><h4>项目群 · 评审讨论</h4><p><b>小周：</b>下周的评审会可以提前到周三下午吗？另外上次说的那份报价，今天能发我吗？</p>
<div style="margin-top:14px;border:1px solid #c9d2e0;border-radius:10px;background:#fff;padding:10px 12px;color:#9aa6b8;font-size:13px">回复小周…</div>
<div class="win cc-inline" style="position:absolute;left:300px;top:150px;width:420px">{winh("清影写作", chip("浏览器","")+btn("","sm ghost","x"))}
<div class="win-b" style="gap:8px"><div class="hint-bar cyan"><b>看懂了：</b>想提前评审会，并催报价。</div>{seg(["同意","婉拒","给个替代"],0,"意图")}<div class="verc on"><div class="vh"><b style="color:var(--amber)">版本 1</b><span class="spacer"></span><span class="kbd">Alt+1</span></div><div class="vt">周三下午可以，三点怎么样？报价今天下班前发你。</div></div><div class="toolbar">{btn("写回输入框","primary sm")}{btn("复制","sm")}{btn("再来一版","sm")}<span class="spacer"></span><span class="kbd">Esc</span></div></div></div></div></div>'''
    body = f'<div class="frames">{reply}{result}{polish}</div><div class="frames">{saved}{badge}</div>{page_}'
    return plain('清影写作（桌面端 + 浏览器端）',2640,body)

# ---------------- 扩展弹窗 ----------------
def p_ext_popup():
    def popup(cur_tab, inner, extra=''):
        t = f'<nav class="hnav" role="tablist" style="margin:0 -14px;padding:0 14px">'+''.join(f'<button role="tab" aria-selected="{"true" if i==cur_tab else "false"}">{n}</button>' for i,n in enumerate(["发现","下载","稍后看","音效","工具","设置"]))+'</nav>'
        return f'<div class="win" style="width:360px">{winh("清影", chip(dot()+"已连接 1.15.0","cyan"))}<div class="win-b">{t}{inner}</div></div>'
    disc = popup(0, f'''<div class="note" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">上海夜未央 · 4K 延时 - 哔哩哔哩</div>{btn("发送当前页面","primary block")}{btn("同步此网站的登录 Cookie","block")}<div class="hint-bar cyan">检测到 <b>3</b> 个视频流，在“页面媒体总览”里勾选发送</div>{btn("页面媒体总览 / 批量发送","block")}{btn("视频摘要 / 总结（Alt+Shift+S）","block")}{btn("截图翻译（框选图片里的文字）","block")}{btn("视频字幕层（双语 / 转写 / 导出）","block")}<div class="toolbar">{btn("翻译此页面 / 还原（Alt+A）","sm")}{btn("翻译窗（Alt+D）","sm")}</div><div class="toolbar">{sel(["翻译成：简体中文","English"],0,"翻译成",160)}{sel(["双语对照","只显示译文"],0,"显示",140)}</div>{check("接管浏览器下载（默认关闭）",False)}{check("登录状态变化时自动同步此网站",True)}''')
    dl = popup(1, f'''<div class="toolbar"><span class="note">3 进行中 · 1 失败</span><span class="spacer"></span>{btn("全部暂停","sm")}</div><div class="list"><div class="item" style="flex-direction:column;align-items:stretch;gap:6px"><div class="ttl">【4K 延时】上海 · 夜未央</div>{prog(64)}<div class="meta"><span class="mono">4.8 MB/s</span><span>剩 12 秒</span><span class="spacer"></span>{btn("暂停","sm")}</div></div><div class="item" style="flex-direction:column;align-items:stretch;gap:6px"><div class="ttl">How rockets land themselves</div>{prog(31,"amber")}<div class="meta"><span class="mono">2.1 MB/s</span></div></div></div><div class="toolbar"><span class="note">清影下载队列</span><span class="spacer"></span><a style="color:var(--amber)">在清影里查看</a></div>''')
    au = popup(3, f'''{row("响度标准化","统一不同视频的音量，默认开启",sw(True,"响度"))}{row("在视频胶囊上显示音效状态","鼠标移到视频上时，右下角显示“● 音效 −16 LUFS”",sw(True,"胶囊"))}<div class="verc"><div class="vh">{dot()}<span>正在处理 · 目标 −16 LUFS</span></div><div class="kv2"><dt>输入</dt><dd>{prog(60,"amber")}</dd><dt>输出</dt><dd>{prog(48)}</dd><dt>增益</dt><dd>{prog(30)}</dd></div></div><div class="toolbar">{chip("人声","acc")}{chip("音乐")}{chip("演讲")}{chip("夜间")}</div>''')
    tools = popup(4, f'''<div class="sec-t" style="margin-top:0">粘贴链接解析</div>{field("","粘贴视频链接","link","链接")}{btn("解析并发送","block")}<div class="sec-t">浏览器下载接管</div>{row("接管下载","大文件交给清影",sw(False,"接管"))}<div class="sec-t">低速站点</div><div class="note">在这些网站点“发送当前页面”时，改发页面里识别到的视频流。</div><textarea aria-label="站点">example.com</textarea>''')
    return plain('浏览器扩展 · 弹窗',1400,f'<div class="frames">{cap("发现")}{disc}{cap("下载")}{dl}{cap("音效")}{au}{cap("工具")}{tools}</div>')

# ---------------- 扩展设置：横向导航 ----------------
def p_ext_settings():
    navs = ["连接","摘录","功能开关","网页翻译","写作浮窗","播放增强快捷键","自动同步 Cookie","规则与名单","隐私与权限"]
    def frame(cur, inner, w=720):
        n = '<nav class="hnav" role="tablist">'+''.join(f'<button role="tab" aria-selected="{"true" if i==cur else "false"}">{x}</button>' for i,x in enumerate(navs))+'</nav>'
        return f'<div class="win" style="width:{w}px">{winh("清影 · 扩展设置", chip(dot()+"已连接清影 1.15.0","cyan"))}<div class="win-b" style="gap:14px">{n}{inner}</div></div>'
    conn = frame(0, f'''<div class="hint-bar"><b>还差一步：</b>填好下面的端口和访问令牌，右键菜单和发送按钮才能使用。</div><p class="note">打开电脑上的清影，进入“设置 → 手机与浏览器扩展”，开启服务，把“分享菜单”里的端口和访问令牌复制到这里，再点“保存并测试”。</p><div class="g2"><div><div class="lbl">端口</div>{field("25196","","","端口")}</div><div><div class="lbl">访问令牌（X-Token）</div>{field("••••••••••••","","","令牌")}</div></div><div class="toolbar">{btn("保存并测试","primary")}<span class="note">端口、令牌只保存在本浏览器里。</span></div>''')
    feat = frame(2, f'''<p class="note">“常驻”表示在你打开的每个网页里运行，默认关闭；“无页面注入”的功能只在后台和弹窗里工作。</p><div class="list">'''+''.join(f'<div class="item"><div class="grow"><div class="ttl">{n}</div><div class="meta"><span>{d}</span></div></div>{chip(t,"acc" if t=="常驻" else "")}{sw(on,n)}</div>' for n,d,t,on in [("网页翻译","整页双语对照、划词翻译","常驻",True),("视频摘要","面板、文稿、思维导图","按需",True),("视频字幕层","叠加双语字幕","按需",True),("悬停放大图片","","常驻",False),("无页面注入的下载接管","只在后台工作","无页面注入",False)])+f'''</div>{check("下载完成时弹出通知",True)}''')
    trans = frame(3, f'''<p class="note">整页双语对照、单击 Ctrl 翻译鼠标指着的段落、选中文字用划词工具条的“翻译”、输入框里连按三次空格翻译。</p>{row("默认翻译成","",sel(["简体中文","English","日本語"],0,"目标",150))}{row("显示方式","",seg(["双语对照","只显示译文"],0,"显示"))}{row("翻译服务","多个服务并排对比",sel(["清影内置 AI","DeepL","Google","腾讯翻译"],0,"服务",170))}{row("不翻译的网站","每行一个域名",btn("编辑","sm","edit"))}''')
    write = frame(4, f'''{row("写作浮窗","在输入框旁用清影写作回复和润色",sw(True,"写作"))}{row("快捷键","",'<span class="kbd">Alt</span><span class="kbd">W</span>')}{row("选中文字出现“写”徽标","",sw(True,"徽标"))}{row("默认人设","",sel(["通用助手","技术写作","文案"],0,"人设",150))}{row("写回输入框后","",seg(["保留浮窗","自动关闭"],1,"写回"))}''')
    return plain('浏览器扩展 · 设置（横向导航）',2060,f'<div class="frames">{cap("连接")}{conn}{cap("功能开关")}{feat}</div><div class="frames">{cap("网页翻译")}{trans}{cap("写作浮窗")}{write}</div>')

# ---------------- 扩展页面内的浮层 ----------------
def p_ext_page():
    vid = f'''<div class="fakepage" style="min-height:420px;padding:0"><div style="position:relative;height:300px;background:linear-gradient(160deg,#10224a,#2c4f8f 55%,#e58a3c)"><div style="position:absolute;left:0;right:0;bottom:18px;text-align:center"><span style="background:rgba(6,10,18,.82);color:#fff;padding:5px 12px;border-radius:6px;font-size:15px">夜里的外滩，像一台缓慢运转的巨大钟表。</span><div style="margin-top:4px"><span style="background:rgba(6,10,18,.7);color:#cfd9ee;padding:2px 10px;border-radius:5px;font-size:12px">The Bund at night turns like a slow clock.</span></div></div><div style="position:absolute;right:12px;bottom:56px;display:flex;gap:6px"><span class="cap-pill"><i class="dot"></i>音效 −16 LUFS</span><span class="cap-pill">下载</span><span class="cap-pill">⋮</span></div></div><div style="padding:14px 22px"><h4>【4K 延时】上海 · 夜未央</h4><p>选中这里的文字，会出现划词工具条：<mark style="background:#ffe08a">timelapse</mark></p></div>
<div class="win" style="position:absolute;left:260px;top:318px;width:310px"><div class="win-b" style="padding:8px;flex-direction:row;gap:6px;align-items:center">{btn("翻译","sm")}{btn("查词","sm")}{btn("摘录","sm")}{btn("写作","sm")}{btn("复制","sm ghost")}</div></div></div>'''
    word = f'''<div class="win" style="width:330px">{winh("timelapse", chip("n.","")+btn("","sm ghost","star"))}<div class="win-b"><div class="mono sub-c">/ˈtaɪmlæps/</div><div>延时摄影；间隔拍摄</div><div class="note">“The timelapse shows the whole day in 12 seconds.”</div><div class="toolbar">{btn("收藏到生词本","sm")}{btn("朗读","sm")}</div></div></div>'''
    dock = f'''<div class="win" style="width:420px;min-height:560px">{winh("视频摘要", btn("","sm ghost","x"))}<div class="win-b">{tabs(["摘要","文稿","评论","思维导图","笔记"],0)}<div class="sec-t" style="margin-top:0">一句话总结</div><div class="note">用延时摄影记录上海外滩与陆家嘴一天的光影变化，约 12 分钟。</div><div class="sec-t">章节</div><div class="list" style="gap:6px"><div class="item" style="padding:8px"><span class="mono sub-c">00:00</span><span class="grow">开场：黄昏的外滩</span></div><div class="item" style="padding:8px;border-color:var(--amber-line)"><span class="mono sub-c">03:12</span><span class="grow">夜色里的陆家嘴</span></div><div class="item" style="padding:8px"><span class="mono sub-c">08:40</span><span class="grow">凌晨的菜市场</span></div></div><div class="toolbar">{btn("存进清影的摘录","sm")}{btn("导出 Markdown","sm")}</div></div></div>'''
    sublayer = f'''<div class="win" style="width:330px">{winh("视频字幕层")}<div class="win-b">{row("双语","",sw(True,"双语"))}{row("字号","",stepper(100))}{row("延迟","",stepper("0.0s"))}<div class="toolbar">{btn("转写","sm")}{btn("导出 SRT","sm")}</div></div></div>'''
    ov = f'''<div class="win" style="width:560px">{winh("页面媒体总览", btn("发送所选","sm"))}<div class="win-b"><div class="toolbar">{seg(["全部","图片","视频","音频","视频流"],0,"类型")}<span class="spacer"></span>{check("全选",True)}</div><div class="g4"><div>{thumb("t1","12:48")}</div><div>{thumb("t2")}</div><div>{thumb("t3")}</div><div>{thumb("t4")}</div></div></div></div>'''
    shot = f'''<div class="win" style="width:330px">{winh("截图翻译")}<div class="win-b"><div class="note">拖动框选图片里的文字，松开后识别并翻译，译文贴回图上。</div><div class="toolbar">{btn("取消","sm")}{btn("翻译","primary sm")}</div></div></div>'''
    body = f'<div class="frames"><div style="width:660px">{cap("页面上：视频胶囊 / 字幕层 / 划词工具条")}{vid}</div><div>{cap("查词卡片")}{word}</div><div>{cap("字幕层设置")}{sublayer}</div></div><div class="frames"><div>{cap("视频摘要面板")}{dock}</div><div>{cap("页面媒体总览")}{ov}<div style="height:16px"></div>{cap("截图翻译")}{shot}</div></div>'
    return plain('浏览器扩展 · 页面内浮层',1560,body)

# ---------------- 手机互传 ----------------
def p_phone():
    def phone(dark, state):
        cls = '' if dark else ''
        head = f'<div class="st"><span>9:41</span><span>5G ▮▮▮</span></div><div class="pb"><div style="display:flex;align-items:center;gap:10px"><span class="logo-s">清</span><b style="font-size:16px">隔空传送</b><span class="spacer"></span>{chip(dot()+"已连接","cyan")}</div>'
        if state=='main':
            inner = f'''{tabs(["发送到电脑","电脑发来的"],0)}<div class="big-btn"><span class="ico">{ic(I["img"],22)}</span><div class="grow"><b>照片和视频</b><div class="note">从相册选择，可多选</div></div></div><div class="big-btn"><span class="ico">{ic(I["file"],22)}</span><div class="grow"><b>文件</b><div class="note">文档、压缩包、任何格式</div></div></div><div class="big-btn"><span class="ico">{ic(I["link"],22)}</span><div class="grow"><b>粘贴剪贴板里的内容</b><div class="note">链接电脑会直接开始下载</div></div></div>{field("","粘贴链接，或写点文字…","","内容")}{btn("发送到电脑","primary block")}<div class="note">只能下载电脑上“明确发给手机”的内容，无法浏览电脑里的其他文件。</div>'''
        elif state=='pair':
            inner = f'''<div style="text-align:center;padding:20px 0"><div style="font-size:15px;font-weight:700">请在电脑上点“允许”</div><div class="note" style="margin:8px 0">第一次使用需要配对。电脑上会弹出请求，核对下面的验证码一致后允许，这里会自动继续。</div><div class="mono" style="font-size:34px;letter-spacing:.3em;color:var(--amber);margin:14px 0">482 913</div><div class="note">请核对电脑上显示的验证码</div></div>'''
        else:
            inner = f'''{tabs(["发送到电脑","电脑发来的"],1)}<div class="list"><div class="item"><div style="width:56px">{thumb("t1")}</div><div class="grow"><div class="ttl">夜未央.mp4</div><div class="meta"><span>71 MB</span></div></div>{btn("下载","sm")}</div><div class="item"><div class="grow"><div class="ttl">报价单.pdf</div><div class="meta"><span>1.2 MB</span></div></div>{btn("下载","sm")}</div></div><div class="empty">电脑上还没有发给手机的其他内容。<br>在电脑的清影里点“发送到手机”，或把文件拖进窗口。</div>'''
        return f'<div class="phone" data-theme="{"dark" if dark else "light"}">{head}{inner}</div></div>'
    transfer = f'''<div class="win" style="width:480px">{winh("手机互传", chip(dot()+"服务已开启","cyan")+btn("选项","sm"))}<div class="win-b"><div class="item"><div style="width:110px;height:110px;border-radius:10px;background:repeating-conic-gradient(var(--text) 0 25%,var(--bg0) 0 50%) 0 0/16px 16px;flex:none"></div><div class="grow"><b>手机扫码连接</b><div class="note">用手机相机或微信扫一扫。<br><span class="mono">http://192.168.1.23:4388</span></div><div class="toolbar" style="margin-top:8px">{btn("复制网址","sm")}{btn("换个地址","sm")}</div></div></div><div class="sec-t">发送对象</div><div class="item"><div class="grow"><div class="ttl">请求配对 · 验证码 <span class="mono" style="color:var(--amber)">482 913</span></div></div>{btn("允许","sm")}{btn("拒绝","sm ghost")}</div><div class="item">{check("我的手机 · Pixel 8",True)}<span class="spacer"></span>{chip(dot()+"在线","cyan")}{btn("撤销","sm ghost")}</div><div class="item">{check("浏览器扩展 · Chrome",False)}<span class="spacer"></span>{chip(dot()+"在线","cyan")}</div><div class="note">勾选的设备才会收到你发出的文件和文字。浏览器扩展默认不勾。</div><div class="sec-t">传输记录</div><div class="item"><div style="width:56px">{thumb("t2")}</div><div class="grow"><div class="ttl">IMG_2041.jpg</div><div class="meta"><span>已保存</span><span>手机发来</span></div></div>{btn("打开","sm")}{btn("显示位置","sm")}</div><div class="item"><div class="grow"><div class="ttl">报价单.pdf</div><div class="meta"><span>手机正在下载</span>{prog(60)}</div></div></div><div class="toolbar">{field("","输入文字，或把文件拖进窗口…","","发送内容")}{btn("发送","primary sm")}</div><div class="toolbar"><span class="note">收到的文件保存在 D:\\清影\\手机</span><span class="spacer"></span>{btn("打开文件夹","sm")}{btn("更改…","sm")}{btn("清空记录","sm danger")}</div></div></div>'''
    body = f'<div class="frames">{cap("手机网页 · 深色")}{phone(True,"main")}{cap("配对中")}{phone(True,"pair")}</div><div class="frames">{cap("手机网页 · 浅色（电脑发来的）")}{phone(False,"recv")}{cap("电脑端 · 手机互传面板")}{transfer}</div>'
    return plain('手机互传（手机网页 + 电脑端）',1500,body)

# ---------------- 小窗口 ----------------
def p_small():
    mini = f'''<div class="win" style="width:300px">{winh("清影 · 迷你窗", btn("","sm ghost","x"))}<div class="win-b">{field("","粘贴链接，Enter 解析","link","链接")}<div class="item"><div class="grow"><div class="ttl">【4K 延时】上海 · 夜未央</div>{prog(64)}</div></div><div class="toolbar">{btn("下载","primary sm")}{btn("在清影里打开","sm")}</div></div></div>'''
    flt = f'''<div class="win" style="width:300px;border-radius:999px"><div class="win-b" style="flex-direction:row;align-items:center;padding:10px 16px;gap:10px">{ic(I["dl"],22)}<span class="grow note">拖入链接即下载</span>{btn("译","sm")}{btn("写","sm")}</div></div>'''
    shotb = f'''<div class="win" style="width:420px">{winh("截图翻译")}<div class="win-b" style="flex-direction:row;align-items:center;flex-wrap:wrap">{btn("重新框选","sm")}{btn("复制译图","sm","img")}{btn("钉在屏幕上","sm")}{btn("保存","sm")}<span class="spacer"></span><span class="kbd">Esc</span></div></div>'''
    pin = f'''<div class="win" style="width:260px"><div class="win-b" style="padding:10px"><div class="thumb t5" style="aspect-ratio:4/3"><div style="position:absolute;left:10%;top:30%;padding:3px 8px;border-radius:5px;background:rgba(6,10,18,.8);color:#fff;font-size:12px">接受任务 · 继续</div></div><div class="note">钉住的译图，右键关闭。</div></div></div>'''
    liveov = f'''<div class="win" style="width:420px">{winh("直播字幕条", chip(dot("red")+"录制中","acc"))}<div class="win-b"><div style="padding:8px 12px;border-radius:8px;background:rgba(6,10,18,.85);color:#fff;font-size:16px">欢迎来到今晚的直播间。<div style="font-size:12px;opacity:.7">Welcome to tonight's stream.</div></div></div></div>'''
    pick = f'''<div class="win" style="width:420px">{winh("选择要录制的画面")}<div class="win-b"><div class="g2"><div>{thumb("t1")}<div class="note" style="margin-top:6px">主屏幕</div></div><div>{thumb("t3")}<div class="note" style="margin-top:6px">PotPlayer</div></div></div><div class="toolbar">{btn("取消","sm")}{btn("开始","primary sm")}</div></div></div>'''
    body = f'<div class="frames">{cap("迷你窗")}{mini}{cap("悬浮窗")}{flt}{cap("截图翻译工具条")}{shotb}</div><div class="frames">{cap("钉住的译图")}{pin}{cap("直播字幕条")}{liveov}{cap("选择录制画面")}{pick}</div>'
    return plain('小窗口与悬浮条',900,body)

# ---------------- 手机互聊 ----------------
def p_chat():
    def bub(name, text, me=False):
        who = '' if me else f'<div class="note" style="margin:0 0 3px 4px;font-size:11.5px">{name}</div>'
        st = 'align-self:flex-end;align-items:flex-end' if me else 'align-self:flex-start'
        bg = 'background:var(--amber);color:#1a1100;border-color:transparent;border-radius:16px 4px 16px 16px' if me else 'border-radius:4px 16px 16px 16px'
        return f'<div style="display:flex;flex-direction:column;max-width:84%;{st}">{who}<div class="bubble" style="{bg}">{text}</div></div>'
    def phone(dark):
        head = f'<div class="st"><span>9:41</span><span>5G ▮▮▮</span></div><div class="pb"><div style="display:flex;align-items:center;gap:10px"><span class="logo-s">清</span><b style="font-size:16px;color:var(--text)">隔空传送</b><span class="spacer"></span>{chip(dot()+"已连接","cyan")}</div>'
        msgs = f'<div class="note" style="text-align:center;font-size:11.5px;line-height:1.5">手机互聊已由电脑开启：同一个二维码连进来的手机都能在这里互发文字。消息会经过电脑上的清影，电脑端能看到全部内容。</div>{bub("小米手机","到了吗")}{bub("","马上到，在地铁口",True)}{bub("iPad","我把会议链接发你们")}'
        inner = f'<div style="color:var(--text);display:contents">{tabs(["发送到电脑","电脑发来的","手机互聊"],2,{2:2})}</div><div style="color:var(--text);display:flex;flex-direction:column;gap:10px;min-height:360px;padding:6px 0">{msgs}</div>{field("","发给其他手机…（电脑上能看到这里的内容）","","聊天")}'
        return f'<div class="phone" data-theme="{"dark" if dark else "light"}">{head}{inner}</div></div>'
    pc_side = f'''<div class="item" style="flex-direction:column;align-items:stretch;gap:8px"><div style="display:flex;align-items:center"><b>手机互聊</b><span class="spacer"></span>{sw(True,"互聊")}</div><div class="note">开启后，已配对的手机能在网页里互相发文字，不用经过你操作。消息只在局域网内流转，但要经过这台电脑上的清影转发，所以你在这里能看到全部内容。关闭或退出清影后记录清空。</div></div>'''
    cmsg = lambda n,t: f'<div style="display:flex;gap:10px;align-items:flex-start"><span class="chip">{n[0]}</span><div style="min-width:0"><div class="note" style="font-size:12px;margin-bottom:3px">{n}</div><div class="bubble">{t}</div></div></div>'
    pc = f'''<div class="win" style="width:760px">{winh("手机互传", chip(dot()+"服务已开启","cyan")+btn("选项","sm"))}<div class="win-b"><div style="display:flex;gap:16px"><div style="width:240px;display:flex;flex-direction:column;gap:10px"><div class="note"><b>发送对象</b> 已选 2 / 3</div>{check("iPhone · 在线",True)}{check("Android 手机",True)}{pc_side}</div><div class="grow" style="display:flex;flex-direction:column;gap:10px">{tabs(["传输记录","手机互聊"],1,{1:5})}<div style="display:flex;flex-direction:column;gap:12px;min-height:300px"><div class="note" style="text-align:center">今天 14:12</div>{cmsg("iPhone","到了吗")}{cmsg("Android 手机","马上到，在地铁口")}{cmsg("iPad","我把会议链接发你们")}</div><div style="display:flex;justify-content:space-between"><span class="note">只读：这里显示手机之间的对话，不会发给手机。</span><a class="note" style="color:var(--amber)">清空聊天记录</a></div></div></div></div></div>'''
    flow = '''<svg viewBox="0 0 760 170" role="img" aria-label="手机互聊的消息流向：手机经电脑上的清影服务转发给另一部手机，电脑端同时显示" width="760" height="170" style="color:var(--text)"><defs><marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 10 5 0 10z" fill="currentColor"/></marker></defs><g fill="none" stroke="currentColor" stroke-width="1.5"><rect x="20" y="55" width="150" height="60" rx="12"/><rect x="305" y="35" width="170" height="100" rx="12" stroke="#ffb547"/><rect x="590" y="55" width="150" height="60" rx="12"/><path d="M170 75H300" marker-end="url(#ar)"/><path d="M475 75H585" marker-end="url(#ar)"/></g><g fill="currentColor" font-size="13" text-anchor="middle"><text x="95" y="90">手机 A（发）</text><text x="390" y="75" font-weight="700">电脑上的清影</text><text x="390" y="98" font-size="12" opacity=".75">转发 + 只读显示</text><text x="665" y="90">手机 B（收）</text><text x="235" y="66" font-size="11.5">POST /api/chat</text><text x="530" y="66" font-size="11.5">GET /api/chat</text><text x="380" y="160" font-size="12" opacity=".75">只在局域网内；消息放内存里，关开关或退出即清空</text></g></svg>'''
    body = f'<div class="frames">{cap("手机网页 · 深色（手机互聊页）")}{phone(True)}{cap("手机网页 · 浅色")}{phone(False)}</div><div class="frames">{cap("电脑端 · 左栏开关 + 只读的手机互聊页")}{pc}</div><div class="frames">{cap("消息怎么走（不是手机直连）")}<div style="padding:8px 0">{flow}</div></div>'
    return plain('手机互聊（默认关闭）',1500,body)
