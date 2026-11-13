# 逐页重设计画板：每页 深/浅 × 1280×800 / 900×700
from v2lib import *

SC = 0.66
SL = {'has': '有文件', 'loading': '加载中', 'empty': '空状态', 'filtered': '已筛选', 'chosen': '已选好目标'}
def N(n): return f'<b class="callout-n" style="position:static;display:inline-grid;margin-left:6px;vertical-align:middle">{n}</b>'

def quad(fn, active, states=('', '', '', ''), notes=(), title='', sub='', audit=(), modes=('full',) * 4, fades=(False,) * 4, hs=(800, 800, 700, 700), extra_top='', extra_bottom=''):
    wins = []
    cfg = [('dark', 1280), ('light', 1280), ('dark', 900), ('light', 900)]
    for k, (th, w) in enumerate(cfg):
        wins.append(win(fn(th, w, states[k]), w, hs[k], SC, th, active, modes[k], fades[k], tag_=f'{"深色" if th == "dark" else "浅色"} · {w}×{hs[k]}' + (f' · {SL.get(states[k], states[k])}' if states[k] else '')))
    nt = ''.join(ann(n, t) for n, t in notes)
    body = f'''{extra_top}<div class="rowf" style="gap:32px;align-items:flex-start;flex-wrap:nowrap">{wins[0]}{wins[1]}</div>
<div class="rowf" style="gap:32px;align-items:flex-start;flex-wrap:nowrap">{wins[2]}{wins[3]}<div class="sp-card" style="flex:1;min-width:300px"><h3>改动说明</h3><div class="colf" style="gap:12px">{nt}</div></div></div>{extra_bottom}'''
    return sheet(title, sub, body, audit)

def foot_note(txt): return f'<p class="sp-note" style="max-width:1000px">{txt}</p>'

# ===================== C3 下载队列 =====================
def c_queue(th, w, st):
    n = w < 1000
    def row(t, meta, pct, state, kind):
        chipk = {'dl': ('下载中 30%', 'ok'), 'wait': ('等待', ''), 'fail': ('失败', 'err'), 'done': ('完成', 'ok'), 'pause': ('已暂停', 'warn')}[kind]
        pk = 'err' if kind == 'fail' else ''
        pr = prog(pct, pk) if kind != 'wait' else prog(0)
        acts = {
            'dl': btn('', 'ico', 's', 'pause', label='暂停') + btn('', 'ico', 's', 'play', label='边下边播') + btn('', 'ico', 's', 'more', label='更多'),
            'wait': btn('', 'ico', 's', 'pause', label='暂停') + btn('', 'ico', 's', 'up', label='优先') + btn('', 'ico', 's', 'more', label='更多'),
            'fail': btn('重试', 'pri', 's', 'refresh') + btn('', 'ico', 's', 'more', label='更多'),
            'done': btn('详情', 'sec', 's') + btn('', 'ico', 's', 'more', label='更多'),
            'pause': btn('继续', 'sec', 's', 'play') + btn('', 'ico', 's', 'more', label='更多'),
        }[kind]
        thc = {'dl': 't1', 'wait': 't2', 'fail': 't3', 'done': 't2', 'pause': 't1'}[kind]
        return f'''<div style="display:flex;align-items:center;gap:16px;padding:12px;border-radius:8px;background:var(--cc-card);border:1px solid var(--cc-line)">{thumb(thc, w=96)}<div style="flex:1;min-width:0;display:flex;flex-direction:column;gap:4px"><div class="ell" style="font-weight:500">{t}</div>{pr}<div class="xs ell mono" style="color:{"var(--cc-err-text)" if kind == "fail" else "var(--cc-faint)"}">{meta}</div></div>{chip(*chipk)}<div style="display:flex;gap:4px;align-items:center;flex:none">{acts}</div></div>'''
    rows = [
        row('示例视频 1：一个比较长的标题用来测试单行省略与换行表现 · 1080P', '28.6 MB / 95.4 MB · 2.4 MB/s · 剩余 28 秒', 30, '', 'dl'),
        row('How rockets land themselves · 1080P', '61.2 MB / 120 MB · 3.1 MB/s · 剩余 19 秒', 51, '', 'dl'),
        row('秋天第一锅板栗焖鸡 · 720P', '排队中 · 前面还有 1 个', 0, '', 'wait'),
        row('春分｜城市里的第一场雨 · 1080P', '连接被重置（HTTP 403） · 可续传', 64, '', 'fail'),
        row('Lo-fi 学习歌单 vol.7 · 音频', '已暂停 · 可续传', 22, '', 'pause'),
        row('老城区的一天 · 1080P', '168 MB · 刚刚完成', 100, '', 'done'),
        row('Rust 异步入门 · 1080P', '52 MB · 2 分钟前完成', 100, '', 'done'),
    ]
    menu_open = ''
    if st == '更多菜单已展开':
        menu_open = f'<div style="position:absolute;right:{28 + 12}px;top:{172 + 98}px;z-index:var(--cc-z-pop)">{menu([("边下边播", "play", ""), ("详情", "info", ""), ("优先", "up", ""), "-", ("取消下载", "x", "dng")], 168)}</div>'
    # 统计合并为一条
    tabsx = seg(['全部 7', '进行中 2', '排队·暂停 2', '失败 1', '已完成 2'], 0, 's')
    strip = f'''<div class="pn" style="flex-direction:row;align-items:center;gap:12px;padding:8px 12px;flex-wrap:wrap;row-gap:8px">{tabsx}<span style="flex:1"></span><span class="xs" style="display:flex;gap:8px;align-items:baseline">总速度<b class="disp" style="font-size:var(--cc-fs-lg);color:var(--cc-fg)">4.8 MB/s</b></span>{btn("全部暂停", "sec", "s", "pause")}{btn("", "ico bord", "s", "more", label="更多批量操作")}</div>'''
    dock = f'''<div class="dock">全部完成后 {seg(["不操作", "睡眠", "关机"], 0, "s")}<span style="flex:1"></span><span class="xs">只对本次运行有效</span></div>'''
    body = f'''{topbar("下载队列", chip("2 个下载中", "ok", True))}{strip}<div class="stack" style="gap:8px;flex:1;min-height:0;overflow:hidden;padding-bottom:56px;position:relative">{"".join(rows)}</div><div class="fadeb" style="bottom:45px;height:48px"></div>{dock}{menu_open}'''
    return body

def board_queue():
    notes = [(1, '<strong>统计卡合并</strong>成一条筛选 + 计数，“总速度”变成读数放右侧；6 张卡占的一整行没了，900 宽也不再 4+2 折行留白。'),
             (2, '<strong>主操作是图标按钮</strong>：下载中 = 暂停、边下边播；等待 = 暂停、优先；失败 = “重试”（唯一的主按钮）。其余收进“更多”。悬停有文字提示。'),
             (3, '<strong>语义分开</strong>：琥珀只给“重试”这类要用户动手的主操作；“优先”不再是琥珀文字，是菜单项；“可续传”不再单独做标签，写进状态行。'),
             (4, '<strong>底部渐隐</strong>：列表区底部留出底栏高度（56px）并加 48px 渐隐，最后一行滚到底时完整露出，不再被底栏切掉。'),
             (5, '<strong>“更多”菜单</strong>（浅色 1280 展开）：边下边播 / 详情 / 优先 / 取消下载（危险色，需确认）。'),
             (6, '状态胶囊统一用语义色：下载中、完成 = 成功青；等待 = 灰；已暂停 = 警告；失败 = 危险。进度条失败时变红。')]
    return quad(c_queue, 'queue', ('', '更多菜单已展开', '', ''), notes, '下载队列 · 主操作与底栏', 'C3 · 每行 4–5 个纯文字操作 → 2–3 个图标按钮 + 更多；统计合并；列表底部渐隐，最后一行不再被裁。', (5, 6), hs=(800, 800, 700, 700))

# ===================== C1 设置·AI =====================
def c_ai(th, w, st):
    n = w < 1000
    exp = (st == '展开了解更多')
    pool = field('用 Supadata 取字幕（备用，更快）', '贴 YouTube 等链接时先取现成字幕，没有时退回本机方式。视频网址会发送给 Supadata。', sw(False, 'Supadata'), more=True)
    detail = f'<div class="desc" style="-webkit-line-clamp:unset;display:block;padding:8px 12px;margin:4px 0 8px;border-left:2px solid var(--cc-line-strong)">按“适用任务 → 优先级（数字小的先用）→ 今天用得少的先用”自动挑一个。某个服务额度用完、被限流或出错时自动冷却并换下一个，不用手动切换。免费额度用完的服务会在次日零点（北京时间）自动恢复。密钥加密保存在本机，不进设置文件和备份。{link("收起")}</div>' if exp else ''
    p1 = pn('模型池', f'''<div class="sm {"" if exp else "ln2"}" style="max-width:640px">翻译字幕、做摘要与章节时，按优先级自动挑一个可用的服务；额度用完或限流时自动换下一个。{"" if exp else link("了解更多") + N(1)}</div>{detail}{empty("cpu", "模型池是空的", "添加一个有免费额度的服务就能开始。", btn("添加服务", "pri", "s", "plus"), cmp=True)}<div style="border-top:1px solid var(--cc-line-soft);margin-top:4px">{pool}</div>''', '翻译字幕 · 摘要 · 章节')
    p2 = pn('AI 术语库', f'''{field("术语库", "让专有名词在每次翻译里保持同一个译法。可自建，或从 " + link("公共术语库", ext=True) + " 安装。" + N(2), "", more=False, col=True).replace('<div class="ctl"></div>', '')}<div class="rowf" style="gap:8px;margin:0 0 8px">{btn("新建术语库", "sec", "s")}{btn("新建文件夹", "sec", "s")}{btn("导入 CSV…", "sec", "s")}</div>{field("翻译记忆", "0 条（其中人工改过 0 条）。同一句话不重复翻译。", btn("清空机器译文", "txt", "s"))}''', '', f'{sw(True, "翻译时使用")}<span class="sm">翻译时使用</span>')
    p3 = pn('AI 专家', f'''{field("默认专家", "“通用”适合大多数场景。" + N(3), sel("通用", w=180))}{field("按网站自动选用", "默认专家是“通用”时，按网址换成匹配的专家，例如 github.com → 技术文档。", sw(True, "自动选用"), more=True)}{field("专家仓库", "从 " + link("AI 专家仓库", ext=True) + " 安装别人分享的专家。", btn("新建专家", "sec", "s") + btn("导入", "sec", "s"))}''')
    note = f'<div class="sm" style="display:flex;gap:8px;align-items:center">{i("ok", 14)}所有修改即时保存。{N(4)}</div>'
    return settings('ai', 'AI', f'<div class="stack">{p1}{p2}{p3}</div>', n)

def board_ai():
    notes = [(1, '<strong>说明折叠</strong>：卡片说明最多 2 行，更长的“了解更多”原位展开（浅色 1280 是展开态）。三张卡原来的 11px 灰字长段落，现在每条 ≤ 2 行 12px。'),
             (2, '<strong>链接统一</strong>：“公共术语库”“AI 专家仓库”用 CcLink（强调色 + 下划线 + 外链图标），不再是浏览器默认蓝。'),
             (3, '<strong>专家显示中文名</strong>“通用”，不再露出内部 id <code>general</code>；下拉换成 CcSelect。'),
             (4, '<strong>保存文案与行为一致</strong>：顶部只有“已自动保存”，删掉卡片里“改完点页面右下角保存才会生效”。（需先确认 AiPool 实际是否即时保存，见总览第 8 项。）'),
             (5, '空的模型池用紧凑 CcEmpty（图 + 标题 + 一句话 + 主操作“添加服务”），主按钮只有一个。'),
             (6, '按钮统一 S 号 28px，不再有“新建专家”被截；术语库三个按钮并排、等高。')]
    return quad(c_ai, 'settings', ('', '展开了解更多', '', ''), notes, '设置 · AI', 'C1 · 文字墙压成每条 ≤ 2 行；链接、按钮、保存提示统一。', (1,))

# ===================== C2 设置全局 =====================
def c_gen(th, w, st):
    n = w < 1000
    lay = field('设置页排布', '多张卡片怎么摆：自动平衡让各列一样高；按顺序严格按先后；单列最简单。只影响设置页。', seg(['自动平衡', '按顺序', '单列'], 0, 's'))
    hl = f'<div style="outline:2px solid var(--cc-acc);outline-offset:4px;border-radius:8px">{lay}</div>'
    p1 = pn('外观与行为', f'''{field("主题", "", seg(["跟随系统", "深色", "浅色"], 1, "s"))}{field("界面语言", "English 界面由机器翻译整理。", sel("简体中文", w=140))}{hl if st == "排布移到这里" else lay}{field("悬浮窗", "把链接拖进去就开始下载；双击回到主窗口。", sw(True, "悬浮窗"))}{field("关闭窗口时", "在托盘运行，用于订阅的定期检查。", seg(["最小化到托盘", "退出"], 0, "s"))}''')
    p2 = pn('快捷键', f'''{field("呼出命令面板", "", kbd("Ctrl", "K"))}{field("截图翻译", "", kbd("Ctrl", "Alt", "T"))}{field("剪贴板下载", "", kbd("Ctrl", "Shift", "D"))}''')
    if st == '分类页（无排布开关）':
        cards = f'<div class="stack">{pn("下载位置", field("默认保存到", "", inp("D:/Downloads/清影", w=240, icon="folder")) + field("同时下载数", "", seg(["1", "2", "3", "5"], 2, "s")) + field("文件名模板", "变量：{title} {author} {id}", inp("{title}", w=180)))}</div>'
        return settings('download', '下载', cards, n)
    return settings('general', '通用', f'<div class="stack">{p1}{p2}</div>', n)

def board_global():
    notes = [(1, '<strong>每个分类页右上不再有“// 设置页排布”</strong>。页头只剩分类标题；右上是已自动保存状态胶囊。第一眼看到的是内容，不是版式开关。'),
             (2, '<strong>“设置页排布”搬到“通用 → 外观与行为”</strong>，是一行标准 CcField，带一句说明。深色 1280 是下载分类页（无开关），浅色 1280 是通用页（框出新位置）。'),
             (3, '分类导航固定 200px（900 宽收到 160px），当前分类 = 强调色字 + 浅琥珀底 + 1px 强调线，不再用阴影。'),
             (4, '搜索设置保留，Ctrl K 键帽用 CcKbd；搜索框与分类同宽。'),
             (5, '页面边距 24px（900 宽 20px）；底部渐隐替代原来的“修改即时生效”脚注行，保存状态只在页头一处出现。')]
    return quad(c_gen, 'settings', ('分类页（无排布开关）', '排布移到这里', '分类页（无排布开关）', '排布移到这里'), notes, '设置全局 · 排布开关搬家', 'C2 · 低频的“设置页排布”从 13 个分类页的右上角移到“通用”页。', (4,))

# ===================== C4 设置·安全 =====================
def hotkey(state):
    if state == 'rec':
        return f'<span class="c-input foc" style="width:260px;justify-content:space-between"><span style="color:var(--cc-acc-text);display:flex;align-items:center;gap:8px">{i("kbd", 14)}按下新的组合键…</span><span class="xs">Esc 取消</span></span>'
    if state == 'err':
        return f'<span class="c-input err" style="width:260px">{kbd("Ctrl", "Alt", "T")}<span class="sfx">{btn("修改", "txt", "s")}</span></span>'
    if state == 'off':
        return f'<span class="c-input" style="width:260px"><span class="ph">未设置</span><span class="sfx">{btn("设置", "sec", "s")}</span></span>'
    return f'<span class="c-input" style="width:260px">{kbd("Ctrl", "Shift", "H")}<span class="sfx">{btn("修改", "txt", "s")}{btn("", "ico", "s", "x", label="清除")}</span></span>'

def c_sec(th, w, st):
    n = w < 1000
    lock = pn('应用锁', field('启动时需要密码', '', sw(True, '密码')) + field('自动锁定', '无操作一段时间后锁定。', sel('10 分钟', w=140)) + field('修改密码', '', btn('修改', 'sec', 's')))
    boss = pn('老板键', f'''{field("快捷键", "按下立刻隐藏所有窗口；设置了应用锁时同时锁定。留空表示不启用。" + N(1), hotkey("idle"))}{field("录入中", "", hotkey("rec"))}{field("冲突", "", hotkey("err"), err="Ctrl+Alt+T 已被“截图翻译”使用，换一个组合键。")}<div style="padding-top:4px">{callout("info", "Linux 上暂不支持全局快捷键，这里设置了也不会生效。" + N(2))}</div>''')
    vault = pn('加密保险箱', field('保险箱', '文件加密保存，没有密码无法找回。', chip('已创建 · 已锁定', 'ok', True)) + field('解锁', '', btn('解锁', 'sec', 's', 'lock')) + field('创建新的保险箱', '', btn('创建', 'sec', 's')))
    priv = pn('隐私', field('不保存解析历史', '', sw(False, '历史')) + field('日志脱敏', '日志里不含密钥、Cookie 与完整路径。', sw(True, '脱敏')) + field('清除数据', '历史、缓存、Cookie。', btn('清除…', 'dng', 's')))
    perm = pn('权限', field('允许局域网设备访问', '', sw(True, '局域网')) + field('允许扩展访问', '', sw(True, '扩展')) + field('下载前确认来源', '来自手机或扩展的任务。', sw(False, '确认')))
    cols = f'<div class="cols2 rs">{"<div class=stack>" + lock + boss + "</div>"}{"<div class=stack>" + vault + priv + perm + "</div>"}</div>'
    return settings('security', '安全与隐私', cols, n)

def board_security():
    notes = [(1, '<strong>快捷键录入控件 CcHotkey</strong>：显示成键帽，不再让人手打 <code>CommandOrControl+Shift+H</code>。三种状态：已设置（修改 / 清除）、录入中（强调色焦点环 +“按下新的组合键… Esc 取消”）、冲突（危险色边框 + 一行说明）。'),
             (2, '<strong>文案补全</strong>：平台限制写成一整句放进信息提示，不再被截断成“Linux 不支持”半句。（具体平台限制文案需对照实现确认。）'),
             (3, '<strong>开关与说明对齐</strong>：每一行都是 CcField，标签、说明在左，控件靠右，开关与第一行标签垂直居中；不再左右各对各的。'),
             (4, '<strong>卡片高度</strong>由内容决定，两列各自堆叠，不再强行等高留空；900 宽自动变单列。'),
             (5, '“清除…”是危险型按钮并带确认对话框（CcDialog sm）。')]
    return quad(c_sec, 'settings', ('', '', '', ''), notes, '设置 · 安全与隐私', 'C4 · 老板键改成快捷键录入控件；说明补全；对齐。', (2,))

# ===================== C5 写作窗 =====================
def wframe(content, w, h, th, scale, lab):
    cap = f'<div class="stage-h" style="margin-bottom:8px"><span>{lab}</span></div>'
    return f'''<div style="display:flex;flex-direction:column;flex:none">{cap}<div class="winwrap" data-t="{th}" style="width:{round(w * scale)}px;height:{round(h * scale)}px"><div class="mw" style="display:block;width:{w}px;height:{h}px;transform:scale({scale});background:var(--cc-bg)">{content}</div></div></div>'''

def c_write(th, st):
    tabsb = '<div class="tabs" style="padding:0 16px">' + ''.join(f'<button role="tab" aria-selected="{"true" if k == 1 else "false"}">{t}</button>' for k, t in enumerate(['帮我回', '改写润色', '检查', '翻译 · 更多', '人设'])) + '</div>'
    hd = f'<div style="display:flex;align-items:center;gap:8px;padding:12px 16px;border-bottom:1px solid var(--cc-line);background:var(--cc-side)"><b style="font-size:var(--cc-fs-lg)">清影写作</b><span class="sp" style="flex:1"></span>{chip("固定在最前", "acc")}{btn("", "ico", "s", "set", label="设置")}{btn("", "ico", "s", "x", label="关闭")}</div>'
    acts = f'<div class="colf" style="gap:8px"><div class="lbl">怎么改{N(1)}</div>{seg(["改写", "润色", "更简洁", "缩短", "更礼貌", "更有说服力"], 1, "s")}</div>'
    menu_o = ''
    if st == 'select':
        menu_o = f'<div style="position:absolute;left:16px;top:508px;z-index:var(--cc-z-pop)">{menu([("通用", "check", "sel"), ("小红书", "", "hov"), ("微博",), ("朋友圈",), ("公众号标题",), ("知乎回答",)], 200)}</div>'
    loading = ''
    if st == 'loading':
        loading = f'<div class="colf" style="gap:8px;padding:12px;border:1px solid var(--cc-line);border-radius:8px;background:var(--cc-card)">{sk("92%", 12)}{sk("100%", 12)}{sk("64%", 12)}</div>'
    else:
        loading = f'<div style="padding:12px;border:1px solid var(--cc-line);border-radius:8px;background:var(--cc-card);font-size:var(--cc-fs-md);line-height:1.6">下周的评审会可以提前到周三下午吗？报价今天能发给你。<div class="rowf" style="gap:8px;margin-top:8px">{btn("复制", "sec", "s", "copy")}{btn("替换原文", "txt", "s")}{link("修改")}</div></div>'
    body = f'''<div class="colf" style="gap:16px;padding:16px">{acts}<div class="colf" style="gap:8px"><div class="lbl">原文</div><div class="c-input" style="height:96px;align-items:flex-start;padding:8px 12px"><span>下周评审会提前到周三下午，报价今天发你。</span></div></div>
<div class="rowf" style="gap:12px;flex-wrap:nowrap"><div class="colf" style="gap:4px;flex:1"><div class="lbl">场景{N(2)}</div>{sel("通用", w=None, open_=(st == "select"))}</div><div class="colf" style="gap:4px;flex:1"><div class="lbl">听写</div>{sel("跟随设置")}</div></div>
<div class="rowf" style="flex-wrap:nowrap;gap:8px">{btn("生成", "pri", "", "bolt")}{btn("清空", "txt")}<span style="flex:1"></span>{kbd("Ctrl", "↵")}</div>
<div class="colf" style="gap:8px"><div class="lbl">结果</div>{loading}</div></div>'''
    err = ''
    if st == 'error':
        err = f'<div style="position:absolute;left:16px;right:16px;bottom:16px;z-index:var(--cc-z-toast)">{toast("err", "生成失败", "模型接口没有响应（超时 30 秒）。", link("重试") + link("查看详情"))}</div>'
    foot = f'<div style="position:absolute;left:0;right:0;bottom:0;padding:8px 16px;border-top:1px solid var(--cc-line);background:var(--cc-side);font-size:var(--cc-fs-sm);color:var(--cc-mute)">只用你给的条件，不替你编理由、不多承诺。</div>'
    return f'<div style="position:relative;height:100%;width:100%">{hd}{tabsb}{body}{foot}{menu_o}{err}</div>'

def board_write():
    W, H, S = 480, 720, 0.8
    ws = [wframe(c_write('dark', ''), W, H, 'dark', S, '深色 · 480×720 · 改写润色'), wframe(c_write('light', ''), W, H, 'light', S, '浅色 · 480×720 · 改写润色'),
          wframe(c_write('dark', 'select'), W, H, 'dark', S, '深色 · 场景下拉已展开'), wframe(c_write('light', 'loading'), W, H, 'light', S, '浅色 · 生成中（骨架）'), wframe(c_write('dark', 'error'), W, H, 'dark', S, '深色 · 失败（带重试）')]
    notes = [(1, '<strong>两层 tab 合并</strong>：顶部只留一层 5 个 tab（帮我回 / 改写润色 / 检查 / 翻译·更多 / 人设）；原来的第二层（改写、润色、更简洁…）变成内容里的 CcSegmented“怎么改”。一层是页面，一层是动作，外观不再相同。'),
             (2, '<strong>原生 select → CcSelect</strong>：“场景”“听写”与 Element 输入同高同圆角、同聚焦环；菜单样式统一（第三张图）。'),
             (3, '“修改”从默认橙色下划线变成 CcLink；结果区的“复制 / 替换原文”是 S 号次按钮。'),
             (4, '禁用的“生成”不再是淡琥珀：禁用色对比度 ≥ 4.5，并在旁边写原因（“先填原文”）。'),
             (5, '生成中用骨架；失败用带“重试 / 查看详情”的 Toast，不自动消失。'),
             (6, '写作窗是独立小窗口，所以画 480×720，不画 1280/900；深浅色各一张，浅色版本之前缺失。')]
    nt = ''.join(ann(n, t) for n, t in notes)
    body = f'''<div class="rowf" style="gap:32px;align-items:flex-start;flex-wrap:nowrap">{ws[0]}{ws[1]}{ws[2]}</div><div class="rowf" style="gap:32px;align-items:flex-start;flex-wrap:nowrap">{ws[3]}{ws[4]}<div class="sp-card" style="flex:1;min-width:360px"><h3>改动说明</h3><div class="colf" style="gap:12px">{nt}</div></div></div>'''
    return sheet('写作窗 · 一层 tab 与 CcSelect', 'C5 · 去掉原生 select 和双层 tab；补上浅色版。', body, (16,))

# ===================== C6 工具箱首页 =====================
def c_tools(th, w, st):
    n = w < 1000
    quick = [('剪辑', '时间线剪辑', 'film'), ('截图', '区域 / 窗口', 'camera'), ('转 GIF', '片段转动图', 'gif'), ('压缩视频', '缩小体积', 'zip')]
    qc = ''.join(f'<div class="pn" style="flex-direction:row;align-items:center;gap:12px;padding:12px 16px;{("box-shadow:inset 0 0 0 1px var(--cc-acc-line);") if k == 0 else ""}"><span style="width:36px;height:36px;border-radius:8px;display:grid;place-items:center;background:var(--cc-acc-soft);color:var(--cc-acc-text);flex:none">{i(ic_, 20)}</span><div style="min-width:0"><div style="font-weight:500">{t}</div><div class="xs">{d}</div></div></div>' for k, (t, d, ic_) in enumerate(quick))
    quickrow = f'<div class="colf" style="gap:8px"><div class="lbl">常用工具{N(1)}</div><div class="cols2" style="grid-template-columns:repeat({2 if n else 4},minmax(0,1fr))">{qc}</div></div>'
    groups = [('剪辑', ['剪辑（时间线）', '快速处理', '转 GIF', '截图']), ('压制与转换', ['压缩视频', '转换格式', '竖屏转横屏']), ('画面与调色', ['视频规整', 'LUT 工作室', '防抖']), ('声音', ['响度标准化', '音效 / 混响', '音频标签']), ('字幕与 AI', ['字幕校对与调整', '生成字幕', 'AI 纠错', '摘要与章节'])]
    nav = ''.join(f'<div class="lbl" style="margin:12px 0 4px;padding-left:12px">{g}</div>' + ''.join(f'<div class="cat" aria-current="{"true" if x == "快速处理" else "false"}" style="height:32px;font-size:var(--cc-fs-sm)"><span class="ell">{x}</span></div>' for x in ts) for g, ts in groups)
    nav = f'<aside class="cats" style="width:200px;flex:none">{inp("", "搜索工具…", "search", size="s")}{nav}<div style="margin-top:8px" class="xs">{N(2)} 分组色点去掉：只有选中项用强调色</div></aside>'
    nofile = (st != 'has')
    cta = f'''<div class="rowf" style="gap:12px;flex-wrap:nowrap;margin-top:8px">{btn("开始处理", "pri", "", "play", st="dis" if nofile else "")}{btn("加入队列", "sec", "", "plus", st="dis" if nofile else "")}{(f'<span class="sm" style="display:flex;gap:4px;align-items:center;color:var(--cc-fg2)">{i("info", 14)}先把文件拖进文件篮，或点“添加文件”{N(3)}</span>') if nofile else chip("预览：实时", "ok", True)}</div>'''
    params = pn('快速处理', f'''<div class="sm">裁剪 · 拼接 · 倍速 · 旋转 · 静音</div><div style="margin:8px 0">{seg(["裁剪", "拼接", "倍速", "旋转", "静音"], 0)}</div>{field("入点 / 出点", "也可以拖动预览下方的滑块。", inp("00:00:12.0", w=100) + inp("00:01:05.4", w=100))}{field("保留方式", "", seg(["快速（不重编码）", "精确（重编码）"], 0, "s"))}{field("输出格式", "", sel("与原文件相同", w=180))}{cta}''')
    if nofile:
        basket = pn('文件篮', f'<div class="c-empty" style="padding:16px 8px;border:1px dashed var(--cc-line-strong);border-radius:8px;gap:4px">{i("plus", 20)}<h4 style="font-size:var(--cc-fs-md)">拖入文件</h4><p>支持视频、音频</p>{btn("添加文件", "sec", "s", "folder")}</div>', '0 个文件')
        prev = pn('预览', f'<div class="thumb" style="width:100%;opacity:.4;background:var(--cc-fill)"></div><div class="sm">添加文件后在这里预览，改参数立刻重画一帧。</div>')
    else:
        basket = pn('文件篮', f'<div class="colf" style="gap:8px">' + ''.join(f'<div style="display:flex;gap:8px;align-items:center">{thumb(t, w=56)}<span class="ell" style="flex:1">{nm}</span>{btn("", "ico", "s", "x", label="移除")}</div>' for t, nm in [('t1', '夜未央.mp4'), ('t3', '苏州河清晨.mp4')]) + '</div>', '2 个文件')
        prev = pn('预览', f'{thumb("t1", w=None).replace("<div class=", "<div style=width:100% class=", 1) if False else thumb("t1")}<div class="rowf" style="margin-top:8px">{seg(["实时", "试一小段", "不需要"], 0, "s")}</div>')
    if n:
        right = ''
        main = f'<div class="stack" style="flex:1;gap:16px">{quickrow}{basket}{params}</div>'
        navtop = f'<div class="rowf" style="gap:8px">{sel("工具：快速处理", w=240)}{N(4)}<span class="xs">窄屏时左侧分组收成下拉</span></div>'
        inner = f'<div class="stack" style="gap:16px;flex:1;min-height:0;overflow:hidden">{navtop}{quickrow}{basket}{params}</div>'
    else:
        inner = f'<div style="display:flex;gap:20px;flex:1;min-height:0;overflow:hidden">{nav}<div class="stack" style="flex:1;gap:16px">{quickrow}{params}</div><div class="stack" style="width:280px;flex:none;gap:16px">{basket}{prev}</div></div>'
    return topbar('工具箱', chip('主导航可收成图标条')) + inner + '<div class="fadeb"></div>'

def board_tools():
    notes = [(1, '<strong>高频工具快捷卡</strong>：剪辑、截图、转 GIF、压缩视频四个入口放在首页最上，一次点击进入；不新增侧栏一级入口。其余工具仍在左侧分组里。'),
             (2, '<strong>分组色点去掉</strong>（蓝/橙/紫/绿/粉是全局唯一的彩色，与琥珀 + 青气质冲突）：分组名用 11px 等宽灰标签，选中项用强调色。'),
             (3, '<strong>禁用要说明原因</strong>：没有文件时“开始处理 / 加入队列”禁用，旁边一行“先把文件拖进文件篮，或点‘添加文件’”；文件篮空态有“添加文件”按钮。浅色 1280 是有文件的正常态。'),
             (4, '<strong>900 宽</strong>：左侧分组收成顶部 CcSelect；文件篮挪到参数上方，三列错位问题消失（参数卡与预览卡不再各自一列）。'),
             (5, '1280 下三列：分组 200 / 主区自适应 / 右栏 280，右栏的文件篮与预览顶部与参数卡对齐，卡片同圆角同内边距。')]
    return quad(c_tools, 'tools', ('', 'has', '', 'has'), notes, '工具箱首页 · 常用入口', 'C6 · 常用四工具一键进入；三列对齐；禁用的按钮说明原因。', (7,), hs=(800, 800, 700, 700))

# ===================== C7 侧栏 =====================
def c_side_content(th, w, st):
    return topbar('下载队列', chip('2 个下载中', 'ok', True)) + f'<div class="pn" style="gap:8px;padding:12px 16px"><div class="ell">示例视频 1：一个比较长的标题用来测试省略</div>{prog(40)}<div class="xs mono">28.6 MB / 95.4 MB · 2.4 MB/s</div></div><div class="pn" style="gap:8px;padding:12px 16px"><div class="ell">How rockets land themselves</div>{prog(70)}<div class="xs mono">61.2 MB / 120 MB</div></div>'

def board_side():
    cfgs = [('dark', 900, 700, 'full', False, '深色 · 900×700 · 全展开（刚好放得下）'), ('light', 900, 560, 'full', True, '浅色 · 900×560 · 矮窗口：中间滚动 + 渐隐'), ('dark', 900, 700, 'col', False, '深色 · 手动收起：仅图标 56px'), ('light', 900, 420, 'col', True, '浅色 · 900×420 · 自动仅图标')]
    ws = []
    for th, w, h, mode, fade, lab in cfgs:
        ws.append(win(c_side_content(th, w, ''), w, h, 0.62, th, 'queue', mode, fade, tag_=lab))
    tbl = table(['窗口高度', '侧栏形态', '尺寸', '行为'], [['≥ 700', '图标 + 11px 标签', '宽 88，导航项 52 高', '9 个入口 + 设置 + 监听全部可见，无需滚动'], ['560 – 699', '同上，中间区可滚动', '同上', 'logo 在顶、设置 / 监听钉在底；中间滚动，上下渐隐，滚动条 3px'], ['< 560 或手动收起', '仅图标', '宽 56，导航项 40 高', '悬停 / 聚焦出现提示（右侧，z-tip）；队列徽标保留']])
    notes = [(1, '<strong>现状问题</strong>：900×700 下 logo 被推出视口，侧栏无滚动，底部“下载中 2 · 等…”被截断（审计 #15）。'),
             (2, '<strong>导航项更矮</strong>：64 → 52px，间距 6 → 2px。9 项 + 设置 + 监听在 700 高度下刚好放得下，约剩 64px。'),
             (3, '<strong>页脚短文案</strong>：只留“● 监听”（8 字符内）；“下载中 2”这类状态已经在队列徽标和页头胶囊里，侧栏不再重复。'),
             (4, '<strong>徽标语义</strong>：队列徽标 = 未完成任务数。收件箱类数字（未处理）用不同形状（圆点 + 数字在标签里），避免“3”同时指两件事。'),
             (5, '<strong>手动收起</strong>：侧栏底部（设置上方）一个“收起/展开”图标按钮，记住选择；矮到 < 560 自动仅图标。过渡 320ms，reduced-motion 下无过渡。')]
    nt = ''.join(ann(n, t) for n, t in notes)
    body = f'''<div class="rowf" style="gap:32px;align-items:flex-start;flex-wrap:nowrap">{ws[0]}{ws[1]}{ws[2]}</div><div class="rowf" style="gap:32px;align-items:flex-start;flex-wrap:nowrap">{ws[3]}<div class="colf" style="flex:1;gap:16px"><div class="sp-card"><h3>按窗口高度的三种形态</h3>{tbl}</div><div class="sp-card"><h3>改动说明</h3><div class="colf" style="gap:12px">{nt}</div></div></div></div>'''
    return sheet('侧栏 · 矮窗口可滚动与折叠', 'C7 · 900×700 下不再把 logo 和设置推出视口；页脚缩成一个状态点。', body, (15,))

# ===================== C8 媒体库 =====================
def c_lib(th, w, st):
    n = w < 1000
    cols = 3 if n else 4
    cards = [('【4K 延时】上海 · 夜未央', '光影漫游者', 't1', '12:48', '71 MB', 'MP4'), ('How rockets land themselves', 'Veritasium', 't2', '18:20', '120 MB', 'WebM'), ('秋天第一锅板栗焖鸡', '山野厨房', 't3', '02:14', '24 MB', 'MP4'), ('春分｜城市里的第一场雨', '光影漫游者', 't2', '10:33', '63 MB', 'MP4'), ('Lo-fi 学习歌单 vol.7', 'Chillout', 't3', '54:01', '48 MB', 'MP3'), ('老城区的一天', '光影漫游者', 't1', '31:55', '168 MB', 'MKV'), ('Rust 异步入门', 'Fireship', 't2', '11:12', '52 MB', 'MP4'), ('夜行者', 'Nightwalker', 't3', '08:40', '40 MB', 'MP4')]
    def cd(c, k):
        t, a, th_, d, s, f = c
        return f'<div class="pn" style="padding:8px;gap:8px;{("box-shadow:inset 0 0 0 2px var(--cc-acc);") if k == 0 else ""}">{thumb(th_, d).replace("<div class=", "<div style=width:100% class=", 1)}<div class="ell" style="font-weight:500">{t}</div><div class="xs" style="display:flex;gap:8px;align-items:center">{a}<span>{s}</span>{chip(f)}</div></div>'
    nf = 3 if st == 'filtered' else 0
    pop = ''
    if st == '筛选弹层已展开':
        pop = f'''<div style="position:absolute;left:{28 + 270}px;top:150px;width:340px;z-index:var(--cc-z-pop)" class="c-menu"><div style="height:auto;display:flex;flex-direction:column;align-items:stretch;gap:12px;padding:12px"><div class="colf" style="gap:4px"><span class="lbl">平台</span>{seg(["全部", "B站", "YouTube", "抖音"], 0, "s")}</div><div class="colf" style="gap:4px"><span class="lbl">类型</span>{seg(["全部", "视频", "音频", "图片"], 0, "s")}</div><div class="colf" style="gap:4px"><span class="lbl">排序</span>{sel("最近下载", "s", 180)}</div><div class="colf" style="gap:8px"><label class="rowf" style="gap:8px;flex-wrap:nowrap"><span class="c-sw on" style="width:32px;height:20px"></span><span style="font-size:var(--cc-fs-sm)">只看收藏</span></label><label class="rowf" style="gap:8px;flex-wrap:nowrap"><span class="c-sw" style="width:32px;height:20px"></span><span style="font-size:var(--cc-fs-sm)">只看文件已丢失的</span></label></div><div class="rowf" style="justify-content:flex-end;gap:8px">{btn("重置", "txt", "s")}{btn("应用", "pri", "s")}</div></div></div>'''
    filt_btn = btn('筛选', 'sec', '', 'filter') .replace('</button>', f'<span class="c-chip acc" style="height:18px;padding:0 6px">{2 if st in ("filtered", "筛选弹层已展开") else ""}</span></button>' if st in ('filtered', '筛选弹层已展开') else '</button>')
    tb = f'''<div class="rowf" style="gap:8px;flex-wrap:{"wrap" if n else "nowrap"}">{inp("", "搜索标题或作者", "search", w=260)}{filt_btn}{N(1)}{btn("多选", "sec")}{btn("工具", "sec", "", None, trail="chev")}<span style="flex:1"></span>{seg(["列表", "网格"], 1)}</div>'''
    chips = ''
    if st in ('filtered', '筛选弹层已展开'):
        chips = f'<div class="rowf" style="gap:8px">{chip("平台：B站 ×", "acc")}{chip("类型：视频 ×", "acc")}{link("清除筛选")}<span style="flex:1"></span><span class="sm">共 24 个文件 · 1.9 GB</span>{N(2)}</div>'
    else:
        chips = f'<div class="rowf"><span class="sm">共 482 个文件 · 38.6 GB</span></div>'
    tabsb = '<div class="tabs">' + ''.join(f'<button role="tab" aria-selected="{"true" if k == 0 else "false"}">{t}</button>' for k, t in enumerate(['已下载', '收到的链接', '手机互传', '解析历史'])) + '</div>'
    if st == 'loading':
        grid = f'<div style="display:grid;grid-template-columns:repeat({cols},minmax(0,1fr));gap:16px">' + ''.join(f'<div class="pn" style="padding:8px;gap:8px">{sk("100%", 0, extra="aspect-ratio:16/9;height:auto")}{sk("80%", 14)}{sk("50%", 10)}</div>' for _ in range(cols * 2)) + '</div>'
        chips = f'<div class="rowf"><span class="sm">正在读取媒体库…</span></div>'
    elif st == 'empty':
        grid = f'<div class="pn" style="padding:16px">{empty("lib", "还没下载过文件", "下载完成的视频、音频和图片会出现在这里。", btn("去解析链接", "pri", "", "link"))}</div>{N(3)}'
        chips = ''
    else:
        grid = f'<div style="display:grid;grid-template-columns:repeat({cols},minmax(0,1fr));gap:16px">' + ''.join(cd(c, k) for k, c in enumerate(cards[:cols * 2])) + '</div>'
    return topbar('媒体库', chip('482 个文件')) + tabsb + tb + chips + f'<div style="flex:1;min-height:0;overflow:hidden;position:relative">{grid}</div><div class="fadeb"></div>' + pop

def board_lib():
    notes = [(1, '<strong>筛选收成一个按钮</strong>：5 个下拉 + 2 个复选框原来比内容还重。现在工具条只有 搜索 / 筛选 / 多选 / 工具 / 视图；点“筛选”弹出一块弹层（深色 1280），里面平台、类型、排序、两个开关，底部“重置 / 应用”。'),
             (2, '<strong>已生效的筛选显示为可移除的小标签</strong>（平台：B站 ×），旁边“清除筛选”；按钮上带数字徽标。浅色 1280 展示这个状态。'),
             (3, '<strong>空状态</strong>（深色 900）：CcEmpty，主操作“去解析链接”，文字对比度 ≥ 4.5（原来几乎与白底同色）。<strong>加载状态</strong>（浅色 1280）：骨架卡片，不再整页转圈。'),
             (4, '复选框换成 CcSwitch 小号并统一行距，“只看收藏 / 只看文件已丢失的”间距一致。'),
             (5, '硬编码的 <code>#f5b73b / #e6a23c / #fff</code> 换成 <code>--cc-warn</code> / <code>--cc-on-acc</code>，浅色下收藏星对比度由 1.9 → 6.1。'),
             (6, '900 宽网格自动 3 列，工具条换行而不是裁切。')]
    return quad(c_lib, 'library', ('筛选弹层已展开', 'loading', 'empty', 'filtered'), notes, '媒体库 · 筛选弹层与四种状态', 'C8 · 筛选条收成弹层；补全加载与空状态。', (8,))

# ===================== C9 空状态 =====================
def board_empty():
    items = [('订阅', 'bell', '还没有订阅', '粘贴频道、UP 主或播放列表链接，有新内容时自动下载。', btn('添加订阅', 'pri', '', 'plus'), '原按钮“开启订阅”实际是添加，文案改成动作本身（审计 #11）；下半 70% 空白由居中的空态填满，不再另放说明卡。'),
             ('生词与摘录', 'vocab', '还没有生词', '收藏的生词和摘录会出现在这里，可以导出。', '', '空表只剩表头 + 4 个全 0 统计卡 → 整块替换成空态，统计卡在有数据后才出现（审计 #9）。没有明确下一步就不放主操作。'),
             ('迷你窗', 'mini', '拖入或粘贴链接', '粘贴到上面的输入框，回车即可开始解析。', btn('打开主窗口', 'sec', 's'), '80% 空白的迷你窗：空态居中，最近任务出现后让位（审计 #17）。'),
             ('直播', 'live', '没有正在录制的直播', '在上方粘贴直播间链接即可开始录制。', '', '文案由两句缩成一句；右侧“录制设置”数字步进器触控区放到 36px（审计 #12）。'),
             ('媒体库', 'lib', '还没下载过文件', '下载完成的视频、音频和图片会出现在这里。', btn('去解析链接', 'pri', '', 'link'), '对比度修正：说明用 --cc-mute（≥ 4.5），不再几乎与底色同色。'),
             ('文件翻译', 'file', '拖入 PDF 或字幕文件', '译文会与原文并排显示，可以导出。', btn('选择文件', 'pri', '', 'folder'), '全页空态；有文件后切回三栏。')]
    cells = ''
    for name, icon, t, d, a, note in items:
        cells += f'<div class="colf" style="gap:8px"><div class="rowf"><b>{name}</b></div>{both(empty(icon, t, d, a))}<p class="sp-note">{note}</p></div>'
    spec = f'''<div class="sp-card"><h3>统一的结构</h3><div class="grid3" style="gap:16px"><div class="colf" style="gap:8px">{ann(1, "<strong>图</strong>：64px 圆角块，线性图标 28px，强调色字 + 浅琥珀底；整页一个、卡片内一个、紧凑行 40px。")}{ann(2, "<strong>标题</strong>：16px / 700，是“还没有…”或“没有符合…”，告诉用户现在的状态。")}</div><div class="colf" style="gap:8px">{ann(3, "<strong>说明</strong>：12px，≤ 2 行，最宽 320px，说明“出现什么”或“怎么开始”。")}{ann(4, "<strong>操作</strong>：0–2 个。有明确下一步 = 一个主按钮；还可以带一个文字链。主按钮文字就是动作。")}</div><div class="colf" style="gap:8px">{ann(5, "<strong>替换而非叠加</strong>：空态占据整个列表区，不留空表头、不留 0 值统计卡。")}{ann(6, "<strong>加载在前</strong>：数据没回来先骨架，回来为空才显示空态，避免空态与转圈叠加。")}</div></div></div>'''
    body = f'''{spec}<div class="grid3" style="gap:32px;align-items:start">{cells}</div>'''
    return sheet('空状态统一 · CcEmpty', 'C9 · 订阅、生词本、迷你窗、直播、媒体库、文件翻译，同一结构、同一文案规则。', body, (9, 11, 12, 17))

# ===================== C10 屏幕翻译 =====================
def c_screen(th, w, st):
    n = w < 1000
    chosen = (st == 'chosen')
    modes = [('翻译与显示', '译文贴在原文上'), ('字幕模式', '译文放进字幕条'), ('漫画模式', '逐格识别气泡')]
    mc = ''.join(f'<div class="pn" style="padding:12px 16px;gap:4px;{("box-shadow:inset 0 0 0 1px var(--cc-acc-line);background:var(--cc-acc-soft);") if k == 0 else ""}"><div style="font-weight:500;{"color:var(--cc-acc-text)" if k == 0 else ""}">{t}</div><div class="sm">{d}</div></div>' for k, (t, d) in enumerate(modes))
    if not chosen:
        hero = f'''<div class="pn" style="padding:24px;gap:16px"><div class="rowf" style="justify-content:space-between"><div style="font-size:var(--cc-fs-lg);font-weight:700">翻译哪里？{N(1)}</div>{seg(["听声音", "看屏幕"], 1, "s")}</div><div class="sm">还没有选取目标。选好后，译文会盖在原处。</div><div class="rowf" style="gap:12px;flex-wrap:wrap">{btn("选择目标", "pri", "l", "screen", trail="chev")}<span class="sm">框选区域 · 点选窗口 · 整个屏幕</span></div><div class="rowf" style="gap:8px;border-top:1px solid var(--cc-line-soft);padding-top:12px;flex-wrap:nowrap"><span class="sm" style="white-space:nowrap">或从窗口列表选</span>{sel("游戏 Eldoria（eldoria.exe）", w=260)}{btn("跟随这个窗口", "sec", "s")}</div></div>'''
        go = f'<div class="rowf" style="gap:12px;flex-wrap:nowrap">{btn("开始翻译", "pri", "", "play", st="dis")}<span class="sm" style="display:flex;gap:4px;align-items:center">{i("info", 14)}先选择一个目标{N(2)}</span></div>'
        prev = pn('预览', '<div class="c-empty" style="padding:24px 8px;gap:4px">' + f'<span class="art" style="width:48px;height:48px">{i("screen", 22)}</span><h4 style="font-size:var(--cc-fs-md)">还没有画面</h4><p>选好目标后在这里看到译文效果。</p></div>')
    else:
        hero = f'''<div class="pn" style="padding:24px;gap:16px"><div class="rowf" style="justify-content:space-between"><div style="font-size:var(--cc-fs-lg);font-weight:700">翻译哪里？</div>{seg(["听声音", "看屏幕"], 1, "s")}</div><div class="rowf" style="gap:12px;flex-wrap:nowrap;padding:12px;border-radius:8px;background:var(--cc-fill);border:1px solid var(--cc-line)">{i("screen", 20)}<div style="flex:1;min-width:0"><div style="font-weight:500">Chrome（chrome.exe）</div><div class="xs">跟随窗口 · 刷新 1 秒</div></div>{btn("重新选择", "sec", "s", trail="chev")}</div></div>'''
        go = f'<div class="rowf" style="gap:8px;flex-wrap:nowrap">{btn("开始翻译", "pri", "l", "play")}{btn("试识别一次", "sec")}</div>'
        prev = pn('预览', f'<div class="thumb" style="width:100%;aspect-ratio:16/10"><span style="position:absolute;left:14%;top:22%;padding:4px 10px;border-radius:6px;background:#060a12cc;color:#fff;font-size:var(--cc-fs-sm)">前往王城的路被封锁了</span><span style="position:absolute;left:30%;top:60%;padding:4px 10px;border-radius:6px;background:#060a12cc;color:#fff;font-size:var(--cc-fs-sm)">接受任务 · 继续</span></div><div class="rowf" style="margin-top:8px">{btn("隐藏译文", "sec", "s")}{btn("保存译图", "sec", "s", "img")}</div>', '', chip('翻译中', 'ok', True))
    modes_card = f'<div class="colf" style="gap:8px"><div class="lbl">模式{N(3)}</div><div class="cols2" style="grid-template-columns:repeat(3,minmax(0,1fr))">{mc}</div></div>'
    opts = pn('选项', f'<div class="rowf" style="justify-content:space-between;flex-wrap:nowrap"><span class="sm" style="flex:1">简体中文 · 不显示原文 · 跳过数字和符号</span>{btn("展开", "txt", "s", trail="chev")}</div>{N(4)}', '默认折叠')
    left = f'<div class="stack" style="gap:16px;flex:1">{hero}{go}{modes_card}{opts}</div>'
    inner = f'<div class="stack" style="gap:16px;flex:1;min-height:0;overflow:hidden">{hero}{go}{modes_card}{opts}{prev if n else ""}</div>' if n else f'<div style="display:flex;gap:20px;flex:1;min-height:0;overflow:hidden">{left}<div class="stack" style="width:340px;flex:none">{prev}</div></div>'
    return topbar('屏幕翻译', chip('就绪', '' , True)) + inner + '<div class="fadeb"></div>'

def board_screen():
    notes = [(1, '<strong>“选择目标”是首屏唯一的主按钮</strong>（L 号 44px），点开菜单选 框选区域 / 点选窗口 / 整个屏幕。原来一行里 3 种入口并排（框选 / 点选 / 翻译这个窗口）主次不清。“从窗口列表选”降成下面一行次要入口。'),
             (2, '<strong>开始翻译在选好目标前禁用并说明原因</strong>；选好后“选择目标”降为“重新选择”（次按钮），“开始翻译”升为主按钮。任何时刻只有一个主按钮。浅色 1280 是选好后的状态。'),
             (3, '<strong>模式卡紧凑</strong>：3 张一行，标题 14px + 一句 12px，选中卡 = 强调线 + 浅琥珀底；原来是整块带 11px 说明的长卡。'),
             (4, '<strong>低频选项默认折叠</strong>成一行摘要（译成什么语言 / 原文小字 / 跳过规则），首屏不需要滚动就能开始翻译；原三层长卡（1 翻译哪里 / 2 模式 / 页签）压成 3 块。'),
             (5, '右侧预览在没有目标时是空态而不是空白；900 宽预览移到下方。')]
    return quad(c_screen, 'screen', ('', 'chosen', '', 'chosen'), notes, '屏幕翻译 · 首屏主次', 'C10 · 一个主按钮；模式卡紧凑；低频选项折叠。', (10,))

PAGES = [('V2-C1', 'C1 设置 · AI', board_ai), ('V2-C2', 'C2 设置全局', board_global), ('V2-C3', 'C3 下载队列', board_queue), ('V2-C4', 'C4 设置 · 安全', board_security), ('V2-C5', 'C5 写作窗', board_write), ('V2-C6', 'C6 工具箱首页', board_tools), ('V2-C7', 'C7 侧栏', board_side), ('V2-C8', 'C8 媒体库', board_lib), ('V2-C9', 'C9 空状态统一', board_empty), ('V2-C10', 'C10 屏幕翻译', board_screen)]
