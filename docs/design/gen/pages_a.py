from lib import *
P1="D:\\清影\\订阅\\光影漫游者"
P2="D:\\清影\\直播录像"

def p_parselist():
    items = [('B站','【4K 延时】上海 · 夜未央','光影漫游者','12:48','t1',True,'1080P · 71 MB',''),
             ('B站','合集 02：苏州河的清晨','光影漫游者','09:21','t3',True,'1080P · 54 MB',''),
             ('B站','【航拍】黄浦江两岸 8K 素材','光影漫游者','06:02','t5',True,'4K · 190 MB','大会员'),
             ('B站','春分｜城市里的第一场雨','光影漫游者','10:33','t2',False,'1080P · 63 MB','已下载过'),
             ('B站','夜行者：凌晨三点的菜市场','光影漫游者','14:10','t4',True,'720P · 40 MB',''),
             ('B站','老城区的一天（完整版）','光影漫游者','31:55','t6',True,'1080P · 168 MB','')]
    rows = ''
    for pl,t,a,d,th,on,sz,tag in items:
        tg = chip(tag,'amber') if tag=='大会员' else (chip(tag) if tag else '')
        rows += f'<div class="item">{check("",on)}{thumb(th,d)[:0]}<div style="width:120px;flex:none">{thumb(th,d)}</div><div class="grow"><div class="ttl">{t}</div><div class="meta"><span>{a}</span><span>{pl}</span>{tg}</div></div><span class="mono sub-c">{sz}</span>{btn("","sm ghost","play")}</div>'
    left = f'''<section class="panel">
<div class="toolbar">{field("https://space.bilibili.com/384519271/video","","link","链接")}{btn("解析","primary","play")}</div>
<div class="toolbar" style="margin-top:12px">{chip(dot()+"B站 · UP 主空间","cyan")}{chip("已识别为列表","amber")}<span class="spacer"></span>{seg(["单个","批量粘贴"],0,"解析模式")}</div>
</section>
<section class="panel">
<div class="panel-h"><h2>光影漫游者 · 视频</h2><span class="eyebrow">已展开 30 / 共 142 条</span><span class="spacer"></span>{seg(["全部","投稿","合集","系列"],0,"分栏")}</div>
<div class="toolbar" style="margin-bottom:12px">{check("全选",True)}<span class="sub-c">已选 5 项 · 预计 335 MB</span><span class="spacer"></span>{field("","在列表中搜索","search","搜索",220)}{sel(["按发布时间 ↓","按播放量","按时长"],0,"排序")}</div>
<div class="list">{rows}</div>
<div class="toolbar" style="margin-top:14px;justify-content:center"><span class="mono sub-c">已加载 30 条，向下滚动继续读取（每页间隔随机 0.4–1.2 秒）</span></div>
</section>'''
    right = f'''<aside class="stack sticky">
<section class="panel">
<div class="eyebrow">这一批怎么下</div>
<div class="row"><div class="t"><b>每个视频的清晰度</b><small>按默认选项，或逐个选择</small></div><div class="ctl">{sel(["自动 1080P","统一 720P","逐个选择"],0,"清晰度",150)}</div></div>
<div class="row"><div class="t"><b>字幕与弹幕</b><small>中文 SRT + 弹幕 ASS</small></div><div class="ctl">{sw(True,"字幕")}</div></div>
<div class="row"><div class="t"><b>放进子文件夹</b><small>光影漫游者 / 视频</small></div><div class="ctl">{sw(True,"子文件夹")}</div></div>
<div class="row"><div class="t"><b>批次名当文件夹</b><small>D:\\清影\\B站\\光影漫游者</small></div><div class="ctl">{sw(False,"批次名")}</div></div>
<div class="row"><div class="t"><b>只在夜间下载</b><small>23:00 – 06:00，其余时间暂停</small></div><div class="ctl">{sw(False,"夜间")}</div></div>
<div class="lines"><div><span>已选</span><b>5 个视频</b></div><div><span>跳过</span><b>1 个（已下载过）</b></div><div><span>合计</span><b>335 MB · 约 56 秒</b></div></div>
<div class="btns">{btn("加入下载队列（5）","primary block","dl")}{btn("先逐个检查再下载","block")}</div>
</section>
<section class="panel"><div class="panel-h"><h2>批量解析检查</h2></div>
<div class="list"><div class="item">{dot()}<span class="grow">重复链接 0 条</span></div><div class="item">{dot("amber")}<span class="grow">需要登录 1 条（大会员清晰度）</span></div><div class="item">{dot()}<span class="grow">已下载过 1 条，默认跳过</span></div></div></section>
</aside>'''
    body = f'<div class="split r"><div class="stack">{left}</div>{right}</div>'
    css = '.stack{display:flex;flex-direction:column;gap:18px;min-width:0}.btns{display:flex;flex-direction:column;gap:10px}.lines{display:flex;flex-direction:column;gap:7px;margin:14px 0;padding:12px 0;border-top:1px solid var(--line);border-bottom:1px solid var(--line);font-size:13px}.lines div{display:flex;justify-content:space-between;color:var(--mute)}.lines b{color:var(--text);font-weight:500}'
    return page('解析 · 列表与批量','parse',1240,f'<span class="spacer"></span>{chip(dot()+"本机服务 已开启","cyan")}',body,css)

def p_queue():
    def job(t, st, p, meta, c='', th='t1', btns=''):
        return f'<div class="item">{check("",False)}<div style="width:110px;flex:none">{thumb(th)}</div><div class="grow"><div class="ttl">{t}</div><div class="meta">{meta}</div><div style="margin-top:8px">{prog(p,c)}</div></div><div style="width:92px;text-align:right">{chip(st,"cyan" if st=="下载中" else ("amber" if st=="暂停" else ""))}</div>{btns}</div>'
    b = lambda *a: ''.join(btn('','sm ghost',i) for i in a)
    jobs = ''.join([
      job('【4K 延时】上海 · 夜未央','下载中',64,'<span>B站</span><span>1080P · MP4</span><span class="mono">4.8 MB/s</span><span class="mono">剩 12 秒</span>','', 't1', b('pause','x')),
      job('How rockets land themselves','下载中',31,'<span>YouTube</span><span>1080P · WebM</span><span class="mono">2.1 MB/s</span><span class="mono">剩 1 分 20 秒</span>','','t5',b('pause','x')),
      job('播放列表 · 38 个视频（12 / 38）','下载中',32,'<span>YouTube</span><span>列表</span><span class="mono">第 12 个 · 1.3 MB/s</span>','amber','t3',b('pause','x')),
      job('春分｜城市里的第一场雨','暂停',45,'<span>B站</span><span>已暂停，可继续</span>','amber','t2',b('play','x')),
      job('夜行者：凌晨三点的菜市场','等待',0,'<span>B站</span><span>排在第 3 位</span>','','t4',b('bolt','x')),
      job('抖音 · 山野厨房 的视频','失败',72,'<span>抖音</span><span style="color:var(--danger)">下载地址已过期，已自动重试 2 次</span>','red','t6',b('refresh','x')),
    ])
    body = f'''<div class="stats">{stat("3","进行中",True)}{stat("1","等待")}{stat("1","暂停")}{stat("28","完成")}{stat("1","失败")}<div class="stat"><b class="mono" style="font-size:22px">8.2 MB/s</b><span>总速度 · 限速 2 MB/s 时段将在 09:00 生效</span></div></div>
<section class="panel">
<div class="toolbar" style="margin-bottom:14px">{seg(["全部","进行中","等待","暂停","完成","失败"],0,"状态")}{sel(["列表：全部","列表：播放列表 · 38","列表：批次 11-12"],0,"列表",190)}<span class="spacer"></span>{btn("全部暂停","sm","pause")}{btn("全部继续 / 重试","sm","play")}{btn("清除已完成","sm ghost","trash")}</div>
<div class="list">{jobs}</div>
<div class="toolbar" style="margin-top:14px"><span class="lbl">全部完成后</span>{seg(["不操作","睡眠","关机"],0,"完成后")}<span class="spacer"></span><span class="note">只对本次运行有效</span>{btn("显示全部 28 个已完成","sm ghost")}</div>
</section>'''
    return page('下载队列','queue',980,f'<span class="spacer"></span>{chip(dot()+"限速计划：夜间不限速","cyan")}',body)

def p_subs():
    def sub(n, plat, th, ints, last, newc, st='正常', cls=''):
        return f'<div class="item"><div style="width:44px;height:44px;border-radius:50%;flex:none" class="thumb {th}"></div><div class="grow"><div class="ttl">{n}</div><div class="meta"><span>{plat}</span><span>每 {ints} 检查</span><span>上次 {last}</span>{chip("新 "+str(newc),"amber") if newc else ""}</div></div>{chip(st,cls)}{sw(st!="已暂停","启用")}{btn("","sm ghost","more")}</div>'
    items = sub('光影漫游者','B站 UP 主','t1','6 小时','12 分钟前',2,'正常','cyan')+sub('Veritasium','YouTube 频道','t5','12 小时','3 小时前',0,'正常','cyan')+sub('画师 · 云间','Pixiv 画师','t2','24 小时','昨天',5,'正常','cyan')+sub('山野厨房','抖音用户主页','t4','6 小时','连续失败 3 次',0,'已暂停','amber')+sub('Fireship 播放列表','YouTube 播放列表','t3','12 小时','6 小时前',1,'正常','cyan')
    body = f'''<div class="split r"><div class="stack" style="display:flex;flex-direction:column;gap:18px">
<section class="panel"><div class="toolbar" style="margin-bottom:14px">{field("","粘贴频道、UP 主空间、画师页或播放列表链接","link","订阅链接")}{btn("添加订阅","primary","plus")}</div>
<p class="note">关注频道、UP 主、画师、合集，有新内容自动下载。每个订阅单独设置检查间隔（默认 6 小时）、过滤条件、清晰度和保存位置。</p></section>
<section class="panel"><div class="panel-h"><h2>订阅与追更</h2><span class="eyebrow">5 个订阅</span><span class="spacer"></span>{field("","搜索订阅","search","搜索",200)}{btn("立即检查全部","sm","refresh")}</div><div class="list">{items}</div></section></div>
<aside style="display:flex;flex-direction:column;gap:18px" class="sticky">
<section class="panel"><div class="panel-h"><h2>光影漫游者</h2><span class="spacer"></span>{chip("编辑","")}</div>
{row("检查间隔","默认 6 小时",sel(["1 小时","6 小时","12 小时","24 小时"],1,"间隔",110))}
{row("首次订阅","只下载以后的新内容",sel(["只要以后的","最近 10 条","全部"],0,"首次",130))}
{row("标题过滤","包含 / 排除关键词",field("延时, 航拍","","","包含",130))}
{row("清晰度","继承默认选项",sel(["默认","1080P","720P"],0,"清晰度",110))}
{row("保存位置",P1,"")}
{row("下载字幕与弹幕","",sw(True,"字幕"))}
<p class="note" style="margin-top:8px">已下载、已忽略的条目不会重复下载；连续失败会自动暂停并通知。</p></section>
<section class="panel"><div class="panel-h"><h2>最近动态</h2></div><div class="list"><div class="item">{dot()}<span class="grow">光影漫游者 · 新投稿 2 个，已加入队列</span><span class="mono sub-c">12 分钟前</span></div><div class="item">{dot("amber")}<span class="grow">山野厨房 · 暂停：需要重新登录</span><span class="mono sub-c">昨天</span></div></div></section>
</aside></div>'''
    return page('订阅','subs',940,f'<span class="spacer"></span>{chip(dot()+"开启订阅 · 托盘运行","cyan")}',body)

def p_live():
    def room(n, plat, st, th, dur, size):
        rec = st=='录制中'
        return f'<div class="item"><div style="width:120px;flex:none">{thumb(th)}</div><div class="grow"><div class="ttl">{n}</div><div class="meta"><span>{plat}</span>{("<span class=mono>已录 "+dur+" · "+size+"</span>") if rec else "<span>未开播</span>"}</div></div>{chip(dot("red" if rec else "")+st,"amber" if rec else "")}{sw(True,"自动录制")}{btn("停止" if rec else "","sm danger" if rec else "sm ghost","x" if rec else "more")}</div>'
    body = f'''<section class="panel"><div class="toolbar">{field("","粘贴直播间链接（B站、抖音、虎牙、斗鱼、YouTube、Twitch…）","link","直播间链接")}{btn("添加直播间","primary","plus")}</div>
<p class="note" style="margin-top:10px">开播自动录制，下播自动结束。</p></section>
<div class="split r"><section class="panel"><div class="panel-h"><h2>直播间</h2><span class="eyebrow">4 个 · 录制中 2</span></div><div class="list">
{room("深夜电台 · 阿森","B站","录制中","t1","01:12:40","1.4 GB")}{room("舞蹈练习室","抖音","录制中","t2","00:21:05","380 MB")}{room("游戏频道 · Kay","Twitch","未开播","t5","","")}{room("天文台直播","YouTube","未开播","t3","","")}
</div></section>
<aside class="stack sticky" style="display:flex;flex-direction:column;gap:18px"><section class="panel"><div class="panel-h"><h2>录制设置</h2></div>
{row("同时录制上限","超出的排队等待",stepper(3))}
{row("自动删除","天前的录像（0 不删）",stepper(30))}
{row("录制格式","断流时自动续录并分段",sel(["MP4","MKV","TS"],1,"格式",90))}
{row("开播通知","录制开始时提醒",sw(True,"通知"))}
{row("保存位置",P2,"")}</section>
<section class="panel"><div class="panel-h"><h2>磁盘</h2></div>{prog(38,"amber")}<p class="note" style="margin-top:8px">录像占用 128 GB，D 盘剩余 214 GB。空间不足时暂停录制并提示。</p></section></aside></div>'''
    return page('直播','live',860,f'<span class="spacer"></span>{chip(dot("red")+"2 路录制中","amber")}',body)
