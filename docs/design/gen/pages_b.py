from lib import *

def p_screen():
    left = f'''<section class="panel"><div class="panel-h"><h2>1. 翻译哪里</h2><span class="spacer"></span>{seg(["听声音","看屏幕"],1,"来源")}</div>
{row("取景方式","“跟随窗口”先在下拉框里选窗口；框选后译文会盖在原处",seg(["框选区域","跟随窗口","整个屏幕"],0,"取景方式"))}
{row("翻译这个窗口","或从窗口列表选",sel(["游戏 Eldoria（eldoria.exe）","Chrome（chrome.exe）","PotPlayer（PotPlayerMini64.exe）"],0,"窗口",260)+btn("框选","sm","screen"))}
{row("刷新速度","越快越费资源",sel(["0.5 秒","1 秒","2 秒"],1,"刷新",100))}
{row("术语库 / 专家","按当前软件自动匹配",sel(["自动匹配","游戏本地化","编程文档"],0,"术语库",150))}
<div class="toolbar" style="margin-top:10px">{btn("试识别一次","sm")}{btn("开始翻译","primary sm","play")}<span class="note">没认出文字时可以换识字引擎，或检查识字语言是否装了。</span></div></section>
<section class="panel"><div class="panel-h"><h2>2. 模式</h2><span class="spacer"></span>{seg(["翻译与显示","字幕模式","漫画模式"],0,"模式")}</div>
{row("译成什么语言","留空 = 用“设置 → AI”里的目标语言",field("简体中文","","","目标语言",160))}
{row("同时显示原文（小字）","",sw(False,"原文"))}
{row("数字、符号、已经是目标语言的文字不翻","省请求，也避免把专有名词翻坏",sw(True,"跳过"))}
{row("图标不识别、不翻译","人像、齿轮、铃铛这类图标常被认成“8”“©”“Q”",sw(True,"图标"))}
{row("清影悬浮窗里放屏幕翻译控制","桌面悬浮窗多一个“译”按钮",sw(True,"悬浮窗"))}
<div class="sec-t">字幕模式</div>
{row("译文放哪里","",seg(["盖在原字幕上","悬浮字幕条"],1,"译文位置"))}
{row("字幕条里同时显示原文","",sw(True,"原文"))}
{row("相似的句子不重复翻","字幕逐字出现、略有改动时只翻最终那句",sw(True,"去重"))}
{row("跨屏合并","一句话分两屏显示时合成一句再翻",sw(False,"合并"))}
<div class="sec-t">漫画模式</div>
{row("识别方式","",seg(["视觉模型（推荐）","本机识字 + 文字翻译"],0,"识别"))}
{row("阅读方向","",seg(["从右到左","从左到右"],0,"方向"))}</section>'''
    right = f'''<aside style="display:flex;flex-direction:column;gap:18px" class="sticky">
<section class="panel"><div class="panel-h"><h2>预览</h2><span class="spacer"></span>{chip(dot()+"翻译中","cyan")}</div>
<div class="thumb t5" style="aspect-ratio:16/10"><div style="position:absolute;left:14%;top:22%;padding:4px 10px;border-radius:6px;background:rgba(6,10,18,.8);color:#fff;font-size:13px">前往王城的路被封锁了</div><div style="position:absolute;left:30%;top:60%;padding:4px 10px;border-radius:6px;background:rgba(6,10,18,.8);color:#fff;font-size:13px">接受任务 · 继续</div></div>
<div class="toolbar" style="margin-top:12px">{btn("开始 / 停止","sm")}{btn("重新框选","sm")}{btn("隐藏译文","sm")}{btn("保存译图","sm","img")}</div>
<p class="note" style="margin-top:10px">译文贴字层不会出现在系统截图里；想留带译文的图，用“保存译图”，存到“下载文件夹/屏幕翻译截图”并复制到剪贴板。</p></section>
<section class="panel"><div class="panel-h"><h2>快捷键</h2></div>{row("开始 / 停止","",'<span class="kbd">Ctrl</span><span class="kbd">Alt</span><span class="kbd">S</span>')}{row("重新框选","",'<span class="kbd">Ctrl</span><span class="kbd">Alt</span><span class="kbd">R</span>')}{row("显示 / 隐藏译文","",'<span class="kbd">Ctrl</span><span class="kbd">Alt</span><span class="kbd">H</span>')}{row("切换模式","",'<span class="kbd">Ctrl</span><span class="kbd">Alt</span><span class="kbd">M</span>')}
<p class="note">游戏如果是“独占全屏”，系统截不到画面，请改成“无边框窗口”或“窗口化”。</p></section>
</aside>'''
    ocr = f'''<section class="panel"><div class="panel-h"><h2>识字</h2><span class="eyebrow">OCR</span></div><div class="g3">
<div class="stat"><b style="font-size:18px">Windows 内置</b><span>识字方式 · 日语已装</span></div>
<div class="stat on"><b style="font-size:18px">PaddleOCR 增强</b><span>已安装 · 中英文及数字，竖排也能认</span></div>
<div class="stat"><b style="font-size:18px">云端识字</b><span>OCR.space / 百度 / Google Vision</span></div></div>
<div class="g2" style="margin-top:14px"><div>{row("识字语言","只能识别 Windows 已安装的语言",sel(["自动","日语（ja）","韩语（ko）"],0,"识字语言",150))}{row("语言包","自动按识字语言选",sel(["自动","一律默认中英文","固定用：日文假名"],0,"语言包",170))}</div><div>{row("增强识字组件","约 15 MB",btn("卸载","sm danger"))}{row("云端识字服务","截图会发给这家服务，密钥只存在本机",sel(["未启用","OCR.space","百度智能云"],0,"云端",150))}</div></div></section>'''
    body = f'<div class="split r"><div style="display:flex;flex-direction:column;gap:18px">{left}</div>{right}</div>{ocr}'
    return page('屏幕翻译','screen',1500,f'<span class="spacer"></span>{chip("翻译别的软件和游戏画面上的文字，译文贴在原位，不挡鼠标")}',body)

def p_library():
    def card_(t,a,th,dur,sz,fmt,sel=False):
        return f'<div class="panel" style="padding:10px;{"outline:2px solid var(--amber)" if sel else ""}">{thumb(th,dur)}<div class="ttl" style="margin-top:10px;font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{t}</div><div class="meta sub-c" style="display:flex;gap:10px;margin-top:2px"><span>{a}</span><span>{sz}</span>{chip(fmt)}</div></div>'
    grid = ''.join([card_('【4K 延时】上海 · 夜未央','光影漫游者','t1','12:48','71 MB','MP4',True),card_('How rockets land themselves','Veritasium','t5','18:20','120 MB','WebM'),card_('秋天第一锅板栗焖鸡','山野厨房','t4','02:14','24 MB','MP4'),card_('春分｜城市里的第一场雨','光影漫游者','t2','10:33','63 MB','MP4'),card_('Lo-fi 学习歌单 vol.7','Chillout','t3','54:01','48 MB','MP3'),card_('老城区的一天','光影漫游者','t6','31:55','168 MB','MKV'),card_('Rust 异步入门','Fireship','t5','11:12','52 MB','MP4'),card_('夜行者：凌晨三点的菜市场','光影漫游者','t4','14:10','40 MB','MP4')])
    body = f'''{tabs(["已下载","收到的链接","手机互传","解析历史"],0,{1:3})}
<div class="toolbar">{field("","搜索标题或作者","search","搜索",260)}{btn("打开下载目录","sm","folder")}{btn("多选","sm")}{sel(["工具 ▾","统计与清理","重复文件","字幕搜索","整理文件夹","回收站","导入与备份"],0,"工具",130)}<span class="spacer"></span>{seg(["列表","网格"],1,"视图")}</div>
<div class="toolbar"><span class="lbl">平台</span>{seg(["全部","B站","YouTube","抖音"],0,"平台")}<span class="lbl">类型</span>{seg(["全部","视频","音频","图片"],0,"类型")}<span class="lbl">排序</span>{sel(["最近下载","按大小","按时长"],0,"排序")}<span class="spacer"></span><span class="sub-c">共 482 个文件 · 38.6 GB</span></div>
<div class="g4">{grid}</div>'''
    sheet = f'''<section class="panel"><div class="panel-h"><h2>手机互传</h2><span class="eyebrow">局域网配对</span><span class="spacer"></span>{chip(dot()+"已连接 1 台设备","cyan")}</div>
<div class="g3"><div class="item"><div style="width:96px;height:96px;border-radius:10px;background:repeating-conic-gradient(var(--text) 0 25%,var(--bg0) 0 50%) 0 0/16px 16px;flex:none"></div><div class="grow"><b>手机扫码打开</b><div class="note">同一 Wi-Fi 下，手机扫码后输入 6 位验证码配对。</div></div></div>
<div class="item"><div class="grow"><div class="ttl">我的手机 · Pixel 8</div><div class="meta"><span>已配对</span>{chip("在线","cyan")}</div></div>{btn("允许","sm")}{btn("拒绝","sm ghost")}</div>
<div><div class="lbl" style="margin-bottom:6px">发送对象</div>{check("我的手机 · Pixel 8",True)}<br>{check("浏览器扩展 · Chrome",False)}<div style="margin-top:10px">{field("","输入文字或链接发送到手机","","发送内容")}</div></div></div></section>'''
    sheet2 = f'''<section class="panel"><div class="panel-h"><h2>统计与清理</h2><span class="eyebrow">媒体库工具</span><span class="spacer"></span>{tabs(["统计与清理","重复文件","字幕搜索","整理文件夹","回收站","导入与备份"],0).replace('class="tabs"','class="tabs" style="border:0"')}</div>
<div class="stats">{stat("482","文件")}{stat("38.6 GB","占用",True)}{stat("17","重复文件 · 3.2 GB")}{stat("6","30 天未打开 · 1.1 GB")}{stat("4","回收站 · 220 MB")}</div></section>'''
    return page('媒体库','library',1560,f'<span class="spacer"></span>{chip("482 个文件")}',body+sheet+sheet2)

def p_filetr():
    left = f'''<section class="panel"><div class="empty" style="padding:60px 12px;border-color:var(--amber-line)">{ic(I["file"],34)}<b>把文件拖到这里，或点击选择</b><span class="note">PDF（含扫描版）、Word、PPT、Excel、EPUB、TXT、Markdown、字幕，译完版面和格式不变，可双语对照。<br>图片里的文字请用“屏幕翻译”或“截图翻译”（Ctrl+Alt+T）。</span>{btn("选择文件","primary","plus")}</div></section>
<section class="panel"><div class="panel-h"><h2>任务</h2><span class="eyebrow">3 个</span></div><div class="list">
<div class="item"><div style="width:44px;height:56px;border-radius:6px;background:var(--panel);border:1px solid var(--line-strong);flex:none;display:grid;place-items:center">{ic(I["file"])}</div><div class="grow"><div class="ttl">Annual_Report_2025.pdf</div><div class="meta"><span>42 页 · 英→中</span><span>双语对照</span></div><div style="margin-top:6px">{prog(64,"")}</div></div>{chip("翻译中 64%","cyan")}{btn("","sm ghost","pause")}</div>
<div class="item"><div style="width:44px;height:56px;border-radius:6px;background:var(--panel);border:1px solid var(--line-strong);flex:none;display:grid;place-items:center">{ic(I["file"])}</div><div class="grow"><div class="ttl">Pitch_Deck.pptx</div><div class="meta"><span>28 页 · 英→中</span></div></div>{chip("已完成","")}{btn("预览","sm")}{btn("导出","sm","dl")}</div>
<div class="item"><div style="width:44px;height:56px;border-radius:6px;background:var(--panel);border:1px solid var(--line-strong);flex:none;display:grid;place-items:center">{ic(I["file"])}</div><div class="grow"><div class="ttl">interview.srt</div><div class="meta"><span>1,204 句</span></div></div>{chip("等待","")}{btn("","sm ghost","x")}</div></div></section>'''
    right = f'''<aside style="display:flex;flex-direction:column;gap:18px" class="sticky"><section class="panel"><div class="panel-h"><h2>翻译设置</h2><span class="spacer"></span>{btn("« 收起","sm ghost")}</div>
{row("语言","源语言自动识别",sel(["自动 → 简体中文","英语 → 简体中文","日语 → 简体中文"],0,"语言",170))}
{row("输出","",seg(["仅译文","双语对照"],1,"输出"))}
{row("页范围","全部。如 3-12, 15 或 5-",field("3-12, 15","","","页范围",110))}
{row("术语库 / 专家","沿用你的设置",sel(["自动匹配","财报","法律合同"],0,"术语",120))}
{row("OCR","扫描页先识字",sel(["自动","高精度","关闭"],0,"OCR",100))}
<div class="sec-t">开始前确认</div>
<div class="hint-bar cyan"><b>42 页 · 约 318 段 · 预计约 36 次请求</b><div class="note">估算，实际随内容长短浮动。这份 PDF 有扫描页，要先识字，请求会更多。</div></div>
<div class="toolbar" style="margin-top:12px">{check("预览",True)}{check("排除页眉页脚",True)}{check("译后检查",True)}</div>
<div class="toolbar" style="margin-top:14px">{btn("开始翻译","primary block","play")}</div></section></aside>'''
    body = f'<div class="split r"><div style="display:flex;flex-direction:column;gap:18px">{left}</div>{right}</div>'
    return page('文件翻译','filetr',1020,f'<span class="spacer"></span>{chip("术语和专家沿用你的设置")}',body)

def p_vocab():
    words = [('latency','/ˈleɪtənsi/','延迟；潜伏期','"We need to reduce latency by half."','Veritasium · 火箭回收','●●●○○'),('idempotent','/ˌaɪdɛmˈpoʊtənt/','幂等的','"Make the request idempotent."','Fireship · Rust 异步','●○○○○'),('拾遗','','补充遗漏','"拾遗补阙，在于细节。"','光影漫游者 · 夜未央','●●●●○'),('ephemeral','/ɪˈfɛmərəl/','短暂的，转瞬即逝的','"An ephemeral storage volume."','Fireship','●●○○○')]
    rows = ''.join(f'<div class="tr" style="--cols:28px 150px minmax(0,1fr) 200px 90px"><span>{check("",False)}</span><span><b>{w}</b><div class="mono sub-c">{p}</div></span><span>{m}<div class="sub-c">{s}</div></span><span class="sub-c">{src}</span><span class="mono" style="color:var(--amber)">{lv}</span></div>' for w,p,m,s,src,lv in words)
    body = f'''{tabs(["生词","摘录"],0)}
<div class="stats">{stat("268","全部生词",True)}{stat("14","今天该复习")}{stat("96","已掌握")}{stat("23","本周新增")}</div>
<section class="panel"><div class="toolbar" style="margin-bottom:12px">{seg(["全部","新词","已掌握"],0,"状态")}{sel(["来源：全部视频","Veritasium · 火箭回收","Fireship"],0,"来源",200)}{field("","搜索单词或释义","search","搜索",240)}<span class="spacer"></span>{btn("开始复习（14）","primary","play")}{btn("导出","sm","dl")}</div>
<div class="table">{tr(["28px","150px","minmax(0,1fr)","200px","90px"],["","单词","释义与原句","来源","熟练度"],True)}{rows}</div></section>
<section class="panel"><div class="panel-h"><h2>摘录</h2><span class="eyebrow">与生词同一处管理</span><span class="spacer"></span>{field("","搜索摘录","search","搜索",220)}</div><div class="g2">
<div class="item"><div class="grow"><div style="font-size:15px">"The best way to predict the future is to invent it."</div><div class="meta"><span>Alan Kay</span><span>00:03:21</span>{chip("来自视频文稿")}</div></div>{btn("","sm ghost","more")}</div>
<div class="item"><div class="grow"><div style="font-size:15px">夜里的外滩，像一台缓慢运转的巨大钟表。</div><div class="meta"><span>光影漫游者</span><span>02:10</span>{chip("来自字幕")}</div></div>{btn("","sm ghost","more")}</div></div></section>'''
    return page('生词与摘录','vocab',980,f'<span class="spacer"></span>{chip("今天该复习 14")}',body)

def p_toolbox():
    def g(name,color,tools,cur=None):
        t=''.join(f'<button class="cat" aria-current="{"true" if x==cur else "false"}"><span>{x}</span></button>' for x in tools)
        return f'<div class="sec-t" style="display:flex;align-items:center;gap:8px"><i class="dot" style="background:{color};box-shadow:none"></i>{name}</div>{t}'
    side = f'''<aside class="cats sticky" aria-label="工具分组">{field("","搜索工具…","search","搜索工具")}
{g("剪辑","#6c8cff",["剪辑（时间线）","快速处理","转 GIF","截图"],"快速处理")}{g("压制与转换","#ff9a5c",["压缩视频","转换格式","竖屏转横屏"])}{g("画面与调色","#b58cff",["视频规整","LUT 工作室","防抖"])}{g("声音","#4be3d0",["响度标准化","音效 / 混响","音频标签"])}{g("字幕与 AI","#ff7fbd",["字幕校对与调整","生成字幕","AI 纠错","摘要与章节"])}</aside>'''
    mid = f'''<section class="stack" style="display:flex;flex-direction:column;gap:18px"><section class="panel"><div class="panel-h"><h2>快速处理</h2><span class="eyebrow">裁剪 · 拼接 · 倍速 · 旋转 · 静音</span><span class="spacer"></span>{btn("返回上一页","sm ghost")}</div>
<div class="toolbar" style="margin-bottom:12px">{seg(["裁剪","拼接","倍速","旋转","静音"],0,"工具")}</div>
{row("入点 / 出点","拖动预览下方的滑块也可",field("00:00:12.0","","","入点",120)+field("00:01:05.4","","","出点",120))}
{row("保留方式","",seg(["快速（不重编码）","精确（重编码）"],0,"保留方式"))}
{row("输出格式","",sel(["与原文件相同","MP4","MKV"],0,"格式",160))}
<div class="toolbar" style="margin-top:12px">{btn("开始处理","primary","play")}{btn("加入队列","","plus")}<span class="spacer"></span>{chip("预览：实时","cyan")}</div></section>
<section class="panel"><div class="panel-h"><h2>任务</h2><span class="eyebrow">JobList</span></div><div class="list"><div class="item"><div class="grow"><div class="ttl">compress · 夜未央.mp4</div><div class="meta"><span>1080P → 720P</span></div><div style="margin-top:6px">{prog(52,"amber")}</div></div>{chip("处理中","amber")}</div><div class="item"><div class="grow"><div class="ttl">loudness · interview.wav</div><div class="meta"><span>−16 LUFS</span></div></div>{chip("完成","cyan")}{btn("打开","sm")}</div></div></section></section>'''
    prev = f'''<aside class="stack sticky" style="display:flex;flex-direction:column;gap:18px"><section class="panel"><div class="panel-h"><h2>预览</h2><span class="spacer"></span>{seg(["实时","试一小段","不需要"],0,"预览档位")}</div>{thumb("t1")}<p class="note" style="margin-top:10px">改参数立刻重画一帧。</p></section>
<section class="panel"><div class="panel-h"><h2>文件篮</h2><span class="eyebrow">3 个文件</span></div><div class="list"><div class="item"><div style="width:70px">{thumb("t1")}</div><span class="grow ttl">夜未央.mp4</span>{btn("","sm ghost","x")}</div><div class="item"><div style="width:70px">{thumb("t3")}</div><span class="grow ttl">苏州河清晨.mp4</span>{btn("","sm ghost","x")}</div><div class="empty" style="padding:14px">拖入更多文件</div></div></section></aside>'''
    body = f'<div class="split" style="grid-template-columns:230px minmax(0,1fr) 320px">{side}{mid}{prev}</div>'
    return page('工具箱','tools',1080,f'<span class="spacer"></span>{chip("主导航可收成图标条")}',body)

def p_editor():
    lanes = f'''<div class="lane" style="background:var(--panel)"><div class="lh">{ic(I["x"],14)}</div><span class="mono" style="position:absolute;left:100px;top:11px;font-size:11px;color:var(--faint)">00:00 &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;00:10 &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;00:20 &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;00:30 &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;00:40</span></div>
<div class="lane"><div class="lh">文字 T1</div><span class="clip t" style="left:260px;width:200px">标题：夜未央</span></div>
<div class="lane"><div class="lh">视频 V2</div><span class="clip v" style="left:360px;width:240px">叠加 · 星空</span></div>
<div class="lane"><div class="lh">视频 V1</div><span class="clip v" style="left:100px;width:260px">外滩 · 延时</span><span class="clip v" style="left:362px;width:300px">陆家嘴 · 延时</span></div>
<div class="lane"><div class="lh">音频 A1</div><span class="clip a" style="left:100px;width:560px"><div class="wave"></div></span></div>
<div class="lane"><div class="lh">音乐 A2</div><span class="clip a" style="left:200px;width:460px">Lo-fi bed · −18 dB</span></div>
<div class="playhead" style="left:420px"></div>'''
    body = f'''<div class="toolbar"><span class="mono" style="font-size:18px;color:var(--amber)">00:00:15.4</span>{btn("","sm","play")}{btn("","sm ghost","cut")}<span class="sub-c">选择 / 刀片 / 修剪 / 标记 / 入出点</span><span class="spacer"></span>{chip("联动","")}{chip("吸附","cyan")}{btn("导出","primary sm","dl")}</div>
<div class="split" style="grid-template-columns:minmax(0,1fr) 320px"><div style="display:flex;flex-direction:column;gap:18px">
<section class="panel"><div class="g2" style="grid-template-columns:minmax(0,1.5fr) minmax(0,1fr)"><div>{thumb("t1")}<div class="toolbar" style="margin-top:10px">{btn("监视器","sm")}{btn("电平表","sm")}{btn("波形","sm")}{chip("代理素材 已启用","cyan")}</div></div>
<div><div class="sec-t" style="margin-top:0">转写文稿</div><div class="list" style="gap:6px"><div class="item" style="padding:8px"><span class="mono sub-c">00:12</span><span class="grow">夜里的外滩，像一台缓慢运转的巨大钟表。</span></div><div class="item" style="padding:8px;border-color:var(--amber-line)"><span class="mono sub-c">00:15</span><span class="grow">潮水退去，城市亮起。</span></div><div class="item" style="padding:8px"><span class="mono sub-c">00:19</span><span class="grow">凌晨三点，菜市场醒了。</span></div></div></div></div></section>
<section class="panel"><div class="panel-h"><h2>时间线</h2><span class="eyebrow">多轨 · 5 条轨道</span><span class="spacer"></span>{btn("+ 视频轨","sm")}{btn("+ 音频轨","sm")}{btn("+ 文字轨","sm")}{btn("轨道模板","sm ghost")}{btn("混音器","sm ghost")}</div>
<div class="tl2" style="height:252px">{lanes}</div></section></div>
<aside style="display:flex;flex-direction:column;gap:18px" class="sticky"><section class="panel"><div class="panel-h"><h2>检查器</h2><span class="spacer"></span>{chip("陆家嘴 · 延时")}</div>
<div class="sec-t" style="margin-top:0">画面</div>
{row("速度","曲线 / 关键帧",stepper("1.0×"))}{row("LUT","Teal & Orange",sel(["无","Teal & Orange","Film 400"],1,"LUT",130))}{row("防抖","",sw(True,"防抖"))}
<div class="sec-t">声音</div>
{row("音量","−2.0 dB",field("−2.0","","","音量",80))}{row("混响","人声 / 大厅",sw(False,"混响"))}{row("响度标准化","−16 LUFS",sw(True,"响度"))}
<div class="sec-t">复合片段 / 调整图层</div>{row("复合片段","把选中的片段合成一个",btn("合成","sm"))}{row("调整图层","对下方所有轨道生效",btn("添加","sm"))}</section>
<section class="panel"><div class="panel-h"><h2>导出</h2></div>{row("格式","",sel(["MP4 H.264","MKV","GIF","仅声音"],0,"格式",130))}{row("响度","",sel(["−16 LUFS","−14 LUFS","不处理"],0,"响度",120))}<div class="toolbar" style="margin-top:12px">{btn("导出视频","primary block","dl")}</div></section></aside></div>'''
    return page('剪辑','tools',1160,f'<span class="spacer"></span>{chip("工具箱 · 剪辑（时间线）")}',body)

def p_overlays():
    def box(t, inner, w=''):
        return f'<section class="panel" style="{w}"><div class="panel-h"><h2>{t}</h2></div>{inner}</section>'
    palette = f'''<div style="border:1px solid var(--line-strong);border-radius:14px;overflow:hidden;background:var(--panel2)"><label class="field" style="border:0;border-bottom:1px solid var(--line);border-radius:0;height:50px">{ic(I["search"])}<input type="text" value="下载" aria-label="命令搜索"><span class="kbd">Esc</span></label><div style="padding:8px" class="cats"><button class="cat" aria-current="true">{ic(I["dl"])}<span>打开 下载队列</span><span class="n">G Q</span></button><button class="cat" aria-current="false">{ic(I["folder"])}<span>打开 下载目录</span></button><button class="cat" aria-current="false">{ic(I["play"])}<span>全部继续 / 重试</span></button><button class="cat" aria-current="false">{ic(I["set"])}<span>设置 · 下载</span></button></div></div>'''
    drawer = f'''<div class="list"><div class="hint-bar red"><b>错误</b> 下载地址已过期，已自动重试 2 次。</div><div class="item"><div class="grow"><div class="ttl">抖音 · 山野厨房 的视频</div><div class="meta"><span>失败原因：需要登录</span></div></div>{btn("去登录","sm")}{btn("重试","sm")}</div><div class="item"><div class="grow"><div class="ttl">复制诊断信息</div><div class="meta"><span>不含密钥、Cookie 与完整路径</span></div></div>{btn("复制","sm")}</div></div>'''
    lock = f'''<div style="display:grid;place-items:center;gap:12px;padding:20px 0">{ic(I["lock"],44,1.4)}<b style="font-size:18px">清影已锁定</b><label class="field" style="width:260px">{ic(I["lock"])}<input type="password" value="••••••" aria-label="密码"></label>{btn("解锁","primary")}<span class="note">无操作 10 分钟自动锁定 · 保险箱同步锁定</span></div>'''
    mini = f'''<div style="display:flex;gap:16px;align-items:center;flex-wrap:wrap"><div class="panel" style="width:150px;padding:14px;text-align:center">{ic(I["dl"],28)}<div class="note" style="margin-top:6px">拖入链接即下载</div><div class="toolbar" style="justify-content:center;margin-top:8px">{btn("译","sm")}</div></div><div class="panel" style="flex:1;min-width:240px;padding:14px"><div class="note">屏幕翻译字幕条（可拖动、缩放）</div><div style="margin-top:8px;padding:8px 14px;border-radius:8px;background:rgba(6,10,18,.85);color:#fff;font-size:16px">夜里的外滩，像一台缓慢运转的巨大钟表。<div style="font-size:12px;opacity:.7;margin-top:2px">The Bund at night turns like a slow clock.</div></div></div></div>'''
    onb = f'''<div class="list"><div class="item"><div class="grow"><b>欢迎使用清影</b><div class="note">1 保存位置 · 2 网络与代理 · 3 登录常用网站 · 4 装好 ffmpeg 与 yt-dlp</div></div>{chip("第 2 步 / 4","amber")}</div><div class="hint-bar"><b>免责声明</b> 请只下载你有权保存的内容；清影不提供绕过版权保护的功能。</div><div class="toolbar">{btn("上一步","sm")}{btn("下一步","primary sm")}</div></div>'''
    body = f'<div class="g2">{box("命令面板（Ctrl K）",palette)}{box("任务抽屉与错误提示",drawer)}{box("锁屏",lock)}{box("悬浮窗与字幕条",mini)}{box("首次引导与免责声明",onb)}<section class="panel"><div class="panel-h"><h2>保险箱</h2><span class="eyebrow">设置 → 安全与隐私</span></div>{row("加密保险箱","已创建，已锁定",btn("解锁","sm","lock"))}<div class="g3" style="margin-top:12px"><div class="thumb t2"></div><div class="thumb t6"></div><div class="thumb t4"></div></div></section></div>'
    return page('浮层与小窗','settings',1100,'<span class="spacer"></span>'+chip('命令面板 · 任务抽屉 · 锁屏 · 悬浮窗 · 引导'),body)
