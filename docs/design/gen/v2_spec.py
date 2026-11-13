# 设计系统 v2：总览 / 令牌 / 组件 / 实施清单 / 硬编码色 画板
from v2lib import *

# ---------- 对比度（按设计稿里的实际取值现算） ----------
def _lum(h):
    h = h.lstrip('#'); r, g, b = [int(h[k:k + 2], 16) / 255 for k in (0, 2, 4)]
    f = lambda c: c / 12.92 if c <= .03928 else ((c + .055) / 1.055) ** 2.4
    return .2126 * f(r) + .7152 * f(g) + .0722 * f(b)
def cr(a, b):
    la, lb = _lum(a), _lum(b)
    if la < lb: la, lb = lb, la
    return (la + .05) / (lb + .05)

DARK = dict(card='#0e1625', bg='#060a12', fg='#e8f0fd', fg2='#c2cfe4', mute='#9aabc6', faint='#8396b8', faint_old='#66789a', acc='#ffb547', acc_text='#ffc760', on_acc='#1a1100', acc_strong='#ffb547', ok_text='#4be3d0', err_text='#ff9aa6', info_text='#9cc6ff', warn_text='#f2cf5b')
LIGHT = dict(card='#fbfcfe', bg='#eaf0f7', fg='#0b1424', fg2='#34445f', mute='#4d5f7b', faint='#566884', faint_old='#8393ab', acc='#a65600', acc_text='#8a4a00', on_acc='#ffffff', acc_strong='#a65600', ok_text='#006a60', err_text='#b01f33', info_text='#1a52a6', warn_text='#7a5c00', acc_old='#b45f00')

def contrast_rows():
    rows = []
    def r(name, k, bgk='card', note=''):
        d, l = cr(DARK[k], DARK[bgk]), cr(LIGHT[k], LIGHT[bgk])
        f = lambda v: f'<b class="{"ok" if v >= 4.5 else "bad"}">{v:.2f}</b> {"通过" if v >= 4.5 else "不足"}'
        rows.append([name, code('--cc-' + k.replace('_', '-')), f(d), f(l), note])
    r('正文', 'fg'); r('次要文字', 'mute'); r('说明/占位（新）', 'faint', note='旧值 4.07 / 3.04，不达标')
    r('强调色文字', 'acc_text'); r('成功文字', 'ok_text'); r('危险文字', 'err_text'); r('信息文字', 'info_text'); r('警告文字', 'warn_text')
    d, l = cr(DARK['on_acc'], DARK['acc_strong']), cr(LIGHT['on_acc'], LIGHT['acc_strong'])
    f = lambda v: f'<b class="{"ok" if v >= 4.5 else "bad"}">{v:.2f}</b> {"通过" if v >= 4.5 else "不足"}'
    rows.append(['主按钮文字', code('--cc-on-acc'), f(d), f(l), '浅色主按钮底由 #b45f00 调到 #a65600（原值白字 4.47）'])
    return rows

# ---------- 总览 ----------
def board_overview():
    maprows = [
        ['C1', '设置 · AI：说明文字墙、默认蓝链接、保存提示矛盾', tag('审计 #1'), 'V2-C1 设置 · AI'],
        ['C2', '设置页排布开关占据每个分类页右上', tag('审计 #4'), 'V2-C2 设置全局'],
        ['C3', '下载队列行：文字操作、被底栏裁切、统计卡折行', tag('审计 #5') + tag('审计 #6'), 'V2-C3 下载队列'],
        ['C4', '设置 · 安全：老板键输入截断、文案被截', tag('审计 #2'), 'V2-C4 设置 · 安全'],
        ['C5', '写作窗：原生 select、两层 tab', tag('审计 #16'), 'V2-C5 写作窗'],
        ['C6', '工具箱：高频工具埋得深、三列错位、禁用无说明', tag('审计 #7'), 'V2-C6 工具箱首页'],
        ['C7', '侧栏：900×700 下被推出视口、页脚被截断', tag('审计 #15'), 'V2-C7 侧栏'],
        ['C8', '媒体库：筛选条过重、空/加载态弱', tag('审计 #8'), 'V2-C8 媒体库'],
        ['C9', '空状态各写各的（订阅、生词、迷你窗、直播…）', tag('审计 #9') + tag('审计 #11') + tag('审计 #12') + tag('审计 #17'), 'V2-C9 空状态统一'],
        ['C10', '屏幕翻译首屏：入口主次不清、模式卡 11px', tag('审计 #10'), 'V2-C10 屏幕翻译'],
        ['C11', '无骨架屏、无进出动画、错误只有瞬时 toast', tag('审计 维度·动效与反馈') , 'V2-C11 加载·错误·反馈'],
        ['C12', '硬编码色不随主题变化', tag('审计 #19') + tag('审计 #20'), 'V2-C12 硬编码色对照'],
        ['A', '字号 8 种、圆角 10+、无间距/阴影/动效/z-index 令牌', tag('审计 维度·视觉层级'), 'V2-DS-1 / V2-DS-2'],
        ['B', '按钮 642 处 + 4 套自绘；3 套 segmented；原生 select', tag('审计 维度·控件一致性'), 'V2-CMP-1 / V2-CMP-2'],
        ['D', '实施顺序、工作量、验收', '', 'V2-D 实施清单'],
    ]
    dec = [
        ('警告色是否独立于琥珀', 'A 独立黄（#f2cf5b / #7a5c00），永远带图标；B 直接复用琥珀', '推荐 A：琥珀只给“可操作/选中”，警告与它同色会混淆'),
        ('浅色强调色 #b45f00 → #a65600', 'A 调深一档，主按钮白字达到 4.5:1；B 保持，主按钮改深色字', '推荐 A：只影响浅色，深色不变'),
        ('去掉所有卡片角上的琥珀小装饰', 'A 全部去掉，只保留底部网格背景；B 仅保留在“当前聚焦”卡', '推荐 A：层级靠间距和字号，不靠装饰'),
        ('工具箱高频入口放哪', 'A 首页顶部 4 张快捷卡；B 侧栏一级再加入口', '推荐 A：不增加一级入口，侧栏 9 项已经满'),
        ('设置页排布开关', 'A 移到“通用”页；B 直接删掉，固定自动平衡', '推荐 A：功能保留，只挪位置'),
        ('侧栏矮窗口策略', 'A 自动：够高全展开、矮了中间滚动、更矮仅图标；B 只做手动收起', '推荐 A + 手动收起都做'),
        ('Chinese 小字下限', 'A 11px 仅限徽标/等宽读数，说明最小 12px；B 全部允许 11px', '推荐 A：中文 11px 很吃力'),
        ('设置是否“即时保存”', 'A 全部即时保存，删掉“点保存”提示；B 保留保存按钮并改顶部文案', '不确定：需先确认 AiPool 实际行为，设计按 A 画'),
    ]
    maptbl = table(['#', '问题', '审计编号', '对应画板'], maprows)
    dtbl = table(['要拍板的事', '选项', '推荐'], [[a, b, c] for a, b, c in dec])
    how = '''<ul><li>每张页面画板都有 <b>深色 / 浅色 × 1280 / 900</b> 的示意，按真实比例缩小摆放，放大查看。</li>
<li>画板里的数字圆点和右侧编号说明一一对应；顶部红框标签是审计报告里的证据编号。</li>
<li>所有取值都用仓库已有的 <b>--cc-*</b> 命名；新增的令牌在“设计系统 · 色彩与令牌清单”里有一份可复制的完整列表。</li>
<li>保持不变：信息架构、功能、侧栏 9 个一级入口、深蓝黑 + 琥珀的气质。变化集中在“更克制、更统一”。</li></ul>'''
    body = f'''<div class="grid2" style="grid-template-columns:1.25fr 1fr;align-items:start">
{card('审计问题 → 画板', maptbl)}
<div class="colf">{card('怎么看这套设计稿', how)}{card('需要产品负责人拍板的 8 件事', dtbl)}</div></div>'''
    return sheet('清影 · UI 提升到 8–9 分的设计稿', '目标：把审计的 6.5 分（视觉气质 7、系统化 5.5）提到 8–9 分。先令牌、再组件、最后逐页。以下画板是新增的 v2，旧的全部页面设计保持原样放在上面。', body)

# ---------- 设计系统 1：字号/圆角/间距/阴影/动效/z/焦点 ----------
def curve(x1, y1, x2, y2, name):
    d = f'M0,100 C{x1 * 100:g},{100 - y1 * 100:g} {x2 * 100:g},{100 - y2 * 100:g} 100,0'
    return f'<div class="colf" style="gap:4px;align-items:flex-start"><svg width="92" height="92" viewBox="-6 -6 112 112" role="img" aria-label="{name}缓动曲线"><rect x="0" y="0" width="100" height="100" fill="none" stroke="var(--cc-line-strong)"/><path d="{d}" fill="none" stroke="var(--cc-acc)" stroke-width="3"/></svg><span class="sm">{name}</span></div>'

def board_ds1():
    ty = [('xs', 11, '1.4', '400 / 500', '徽标、等宽读数、键帽、表头', '10 · 10.5 · 11 · 11.5'),
          ('sm', 12, '1.5', '400 / 500', '说明、次要信息、小按钮、表单错误', '12 · 12.5'),
          ('md', 14, '1.5', '400 / 500', '正文、控件、列表标题、表单标签', '13 · 13.5 · 14'),
          ('lg', 16, '1.4', '700', '卡片/对话框/空状态标题', '15 · 16（新增用法）'),
          ('xl', 20, '1.3', '700', '页面标题', '18 · 22'),
          ('2xl', 28, '1.2', '700 · Chakra Petch', '统计大数字、速度读数', '24 · 26 · 28')]
    sample = '清影 ClearClip 28.6 MB / 95.4 MB'
    rows = ''.join(f'<tr><td>{code("--cc-fs-" + n)}</td><td class="mono">{px}px / {lh}</td><td style="font-size:{px}px;color:var(--cc-fg);white-space:nowrap">{sample if px < 20 else "清影 ClearClip" if px == 20 else "4.8 MB/s"}</td><td>{use}</td><td class="mono" style="color:var(--cc-faint)">{old}</td></tr>' for n, px, lh, w, use, old in ty)
    typ = f'<div style="overflow-x:auto"><table class="tbl"><thead><tr><th>令牌</th><th>字号 / 行高</th><th>样例</th><th>用途</th><th>替换旧值</th></tr></thead><tbody>{rows}</tbody></table></div>'
    typ += '<p class="sp-note">字重只用 400 / 500 / 700。<b>不再出现带 .5 的字号</b>；中文说明最小 12px，11px 只给徽标与等宽数字。数字列一律 <code>font-variant-numeric: tabular-nums</code>。</p>'
    rad = ''.join(f'<div class="colf" style="gap:8px;align-items:flex-start"><div style="width:96px;height:64px;border:2px solid var(--cc-acc);border-radius:{r}px;background:var(--cc-acc-soft)"></div><div class="sm"><b style="color:var(--cc-fg)">{n}</b> {v}</div><div class="xs" style="max-width:150px">{u}</div></div>' for n, v, r, u in [('--cc-r-sm', '4px', 4, '标签、键帽、骨架块、小徽标'), ('--cc-r-md', '8px', 8, '按钮、输入、选择、菜单、Toast'), ('--cc-r-lg', '12px', 12, '卡片、对话框、窗口内面板'), ('--cc-r-pill', '999px', 999, '状态胶囊、开关')])
    rad = f'<div class="rowf" style="gap:24px;align-items:flex-start">{rad}</div><p class="sp-note">替换旧值：3 4 5 6 7 8 9 10 12 14 999 共 11 种 → <b>3 档 + pill</b>。按钮从 10px 收到 8px，卡片从 14px 收到 12px。</p>'
    sp = [(4, '--cc-sp-1', '图标与文字间距'), (8, '--cc-sp-2', '控件内、相邻小控件'), (12, '--cc-sp-3', '列表行内边距、字段行上下'), (16, '--cc-sp-4', '卡片之间、卡片内区块'), (20, '--cc-sp-5', '卡片内边距（宽）'), (24, '--cc-sp-6', '页面边距（1280）'), (32, '--cc-sp-8', '大区块'), (40, '--cc-sp-10', '空状态上下'), (48, '--cc-sp-12', '页面顶部留白')]
    spr = ''.join(f'<div style="display:grid;grid-template-columns:90px 36px 1fr;gap:12px;align-items:center;font-size:12px"><span class="mono" style="color:var(--cc-acc-text)">{n}</span><span class="mono">{px}</span><span style="display:flex;align-items:center;gap:12px"><i style="display:block;height:12px;width:{px * 3}px;background:var(--cc-acc-soft);border:1px solid var(--cc-acc-line);border-radius:2px"></i><span style="color:var(--cc-mute)">{u}</span></span></div>' for px, n, u in sp)
    spr += '<p class="sp-note">一切间距是 4 的倍数；900 宽以下页面边距收到 20。不再出现 5 / 6 / 7 / 9 / 10 / 14 / 18 / 22 这类零散值。</p>'
    sh1 = ''.join(f'<div class="colf" style="gap:8px"><div style="height:56px;border-radius:12px;background:var(--cc-card);box-shadow:var({n})"></div><div class="sm"><b style="color:var(--cc-fg)">{n}</b><br>{u}</div></div>' for n, u in [('--cc-sh-1', '卡片静止：1px 暗边'), ('--cc-sh-2', '菜单、下拉、Toast、悬浮条'), ('--cc-sh-3', '对话框、全屏浮层')])
    sh = both(f'<div class="colf" style="gap:20px">{sh1}</div>') + '<p class="sp-note">深色下阴影不明显，所以 sh-2 / sh-3 自带 1px 描边环；浅色同理。替换现有单一的 <code>--cc-shadow</code>。</p>'
    dur = [('--cc-dur-fast', '120ms', '悬停、按下、焦点环、复选'), ('--cc-dur-base', '200ms', '展开折叠、开关、菜单出现、列表行进出'), ('--cc-dur-slow', '320ms', '页面切换、对话框进出、侧栏折叠')]
    bars = ''.join(f'<div style="display:grid;grid-template-columns:130px 52px 1fr;gap:12px;align-items:center;font-size:12px"><span class="mono" style="color:var(--cc-acc-text)">{n}</span><span class="mono">{v}</span><span style="color:var(--cc-mute)">{u}</span></div>' for n, v, u in dur)
    cv = f'<div class="rowf" style="gap:24px;margin-top:8px">{curve(.2, .8, .2, 1, "ease-out · 进入")}{curve(.4, 0, .2, 1, "ease-io · 移动")}{curve(.4, 0, 1, 1, "ease-in · 离开")}</div>'
    mo = bars + cv + '<p class="sp-note">令牌 <code>--cc-ease-out</code> / <code>--cc-ease-io</code> / <code>--cc-ease-in</code>。只动 <b>opacity 与 transform</b>，不动 width/height/top。<code>prefers-reduced-motion</code> 下位移去掉、时长归零，见“加载 · 错误 · 反馈”。</p>'
    zs = [(400, '--cc-z-tip', 'Tooltip'), (300, '--cc-z-toast', 'Toast'), (200, '--cc-z-dialog', '对话框 + 遮罩'), (100, '--cc-z-pop', '下拉 / 菜单 / 筛选弹层'), (20, '--cc-z-side', '侧栏'), (10, '--cc-z-sticky', '吸顶工具条、底栏、渐隐'), (0, '(auto)', '页面内容')]
    zr = ''.join(f'<div style="margin-left:{k * 14}px;display:grid;grid-template-columns:44px 130px 1fr;gap:8px;align-items:center;padding:6px 12px;border:1px solid var(--cc-line-strong);border-radius:8px;background:var(--cc-card);font-size:12px;box-shadow:var(--cc-sh-1)"><b class="mono" style="color:var(--cc-acc-text)">{z}</b><span class="mono">{n}</span><span style="color:var(--cc-mute)">{u}</span></div>' for k, (z, n, u) in enumerate(zs[::-1]))
    zr = f'<div class="colf" style="gap:6px">{zr}</div><p class="sp-note">现有代码里散落的 z-index（9 / 99 / 1000 / 2001…）全部改用这 6 个令牌；Element Plus 的弹层也用它们覆盖。</p>'
    fo = f'''<div class="rowf" style="gap:20px;align-items:flex-start">{btn("主按钮", "pri", st="foc")}{btn("次按钮", "sec", st="foc")}{inp("", "输入框", st="foc", w=150)}<span class="nv" aria-current="page" style="width:64px;height:52px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:4px;border-radius:8px;color:var(--cc-acc-text);background:var(--cc-acc-soft);outline:2px solid var(--cc-focus);outline-offset:2px;font-size:11px">{i("dl", 20)}<span>队列</span></span></div>
<ul><li><b>只在键盘聚焦时显示</b>（<code>:focus-visible</code>），鼠标点击不出现。</li><li>2px 实线，偏移 2px，颜色 <code>--cc-focus</code> = 强调色；对相邻底色 ≥ 3:1（深色 10.3、浅色 5.6）。</li><li>全局一条规则覆盖：<code>:where(button,a,input,select,[tabindex]):focus-visible</code>；不再逐组件写 outline。</li><li>输入框聚焦同时把边框改成强调色；Element Plus 控件用同一令牌覆盖。</li></ul>'''
    body = f'''{sec('字号阶梯 · 6 档', typ, tags(1))}
<div class="grid2" style="align-items:start">{sec('圆角 · 3 档 + pill', card('', rad))}{sec('间距 · 4px 栅格', card('', f'<div class="colf" style="gap:8px">{spr}</div>'))}</div>
<div class="grid2" style="align-items:start">{sec('阴影 · 3 档', card('', sh))}{sec('动效 · 时长与缓动', card('', mo))}</div>
<div class="grid2" style="align-items:start">{sec('z-index · 6 层', card('', zr))}{sec('焦点环', card('', fo))}</div>'''
    return sheet('设计系统 v2 · 度量与动效', '字号、圆角、间距、阴影、动效、层级、焦点环。令牌名一律 --cc-*，加在 theme.css 的 :root（深浅色共用，不随主题变）。', body, audit=('维度·视觉层级与一致性', '维度·动效与反馈'))

# ---------- 设计系统 2：色彩 ----------
def board_ds2():
    def sw_(name, col, soft, line, text, extra=''):
        return f'''<div class="colf" style="gap:8px"><div class="rowf" style="gap:8px"><b style="font-size:14px">{name}</b>{extra}</div>
<div class="grid4" style="grid-template-columns:repeat(4,1fr);gap:8px"><div class="swatch" style="background:var(--cc-{col});color:var(--cc-bg)">{col}</div><div class="swatch" style="background:var(--cc-{soft});color:var(--cc-fg)">{soft}</div><div class="swatch" style="background:var(--cc-card);border-color:var(--cc-{line});color:var(--cc-fg)">{line}</div><div class="swatch" style="background:var(--cc-card);color:var(--cc-{text})">{text}</div></div>
<div class="rowf" style="gap:8px">{chip(name + "标签", col2[col], True)}</div></div>'''
    col2 = {'ok': 'ok', 'info': 'info', 'warn': 'warn', 'err': 'err', 'acc': 'acc'}
    sem = ''.join([
        sw_('success 成功 / 青', 'ok', 'ok-soft', 'ok-line', 'ok-text', tag('已有 + ok-line', 'new')),
        sw_('info 信息', 'info', 'info-soft', 'info-line', 'info-text', tag('NEW', 'new')),
        sw_('warn 警告', 'warn', 'warn-soft', 'warn-line', 'warn-text', tag('NEW', 'new')),
        sw_('danger 危险', 'err', 'err-soft', 'err-line', 'err-text', tag('err 已有；其余 NEW', 'new')),
        sw_('accent 强调（只给可操作 / 选中）', 'acc', 'acc-soft', 'acc-line', 'acc-text', tag('已有', 'mute')),
    ])
    sem_both = both(f'<div class="colf" style="gap:20px">{sem}</div>')
    uses = f'''<div class="grid2" style="gap:16px">{callout("info", "信息：说明背景或操作提示。")}{callout("warn", "警告：会产生影响但可继续，永远带图标。")}{callout("err", "危险：失败或不可恢复，配重试或解决办法。")}{callout("ok", "成功：已完成，不需要用户再做什么。")}</div>'''
    ctbl = table(['场景', '令牌', '深色', '浅色', '备注'], contrast_rows())
    tok = [
        ('字号', '--cc-fs-xs / sm / md / lg / xl / 2xl', '11 / 12 / 14 / 16 / 20 / 28 px'),
        ('行高', '--cc-lh-tight / base', '1.25 / 1.5'),
        ('圆角', '--cc-r-sm / md / lg / pill', '4 / 8 / 12 / 999 px'),
        ('间距', '--cc-sp-1 … 12', '4 8 12 16 20 24 32 40 48 px'),
        ('控件高度', '--cc-h-sm / md / lg', '28 / 36 / 44 px'),
        ('阴影', '--cc-sh-1 / 2 / 3', '见“度量与动效”；深浅各一套'),
        ('遮罩', '--cc-scrim', '#060a12b8 / #0b142459'),
        ('时长', '--cc-dur-fast / base / slow', '120 / 200 / 320 ms'),
        ('缓动', '--cc-ease-out / io / in', 'cubic-bezier(.2,.8,.2,1) / (.4,0,.2,1) / (.4,0,1,1)'),
        ('层级', '--cc-z-sticky / side / pop / dialog / toast / tip', '10 / 20 / 100 / 200 / 300 / 400'),
        ('焦点环', '--cc-focus / --cc-focus-w / --cc-focus-off', '= --cc-acc / 2px / 2px'),
        ('信息色', '--cc-info / -soft / -line / -text', '深 #7db4ff 系 / 浅 #1f5fbf 系'),
        ('警告色', '--cc-warn / -soft / -line / -text', '深 #f2cf5b 系 / 浅 #7a5c00 系'),
        ('危险色补全', '--cc-err-soft / -line / -text', '深 #ff7b8a 系 / 浅 #c4283d 系'),
        ('成功色补全', '--cc-ok-line', '#4be3d066 / #00826666'),
        ('骨架屏', '--cc-skel / --cc-skel-hi', '#16233a #1f3150 / #e0e8f2 #eef3fa'),
    ]
    chg = [('--cc-faint', '#66789a → #8396b8', '#8393ab → #566884', '占位、说明、禁用文字对比度达标'),
           ('--cc-acc / --cc-acc-strong（仅浅色）', '不变 #ffb547', '#b45f00 → #a65600', '浅色主按钮白字 4.47 → 5.6')]
    tt = table(['类别', '新增令牌', '值'], [[a, code(b), c] for a, b, c in tok])
    ct = table(['现有令牌', '深色', '浅色', '原因'], [[code(a), b, c, d] for a, b, c, d in chg])
    css = '''<pre style="margin:0;padding:16px;border-radius:8px;background:var(--cc-fill);border:1px solid var(--cc-line);font-family:var(--cc-mono);font-size:11px;line-height:1.6;color:var(--cc-fg2);overflow-x:auto">:root {
  --cc-fs-xs: 11px; --cc-fs-sm: 12px; --cc-fs-md: 14px; --cc-fs-lg: 16px; --cc-fs-xl: 20px; --cc-fs-2xl: 28px;
  --cc-r-sm: 4px; --cc-r-md: 8px; --cc-r-lg: 12px; --cc-r-pill: 999px;
  --cc-sp-1: 4px; --cc-sp-2: 8px; --cc-sp-3: 12px; --cc-sp-4: 16px; --cc-sp-5: 20px; --cc-sp-6: 24px; --cc-sp-8: 32px;
  --cc-h-sm: 28px; --cc-h-md: 36px; --cc-h-lg: 44px;
  --cc-dur-fast: 120ms; --cc-dur-base: 200ms; --cc-dur-slow: 320ms;
  --cc-ease-out: cubic-bezier(.2,.8,.2,1); --cc-ease-io: cubic-bezier(.4,0,.2,1); --cc-ease-in: cubic-bezier(.4,0,1,1);
  --cc-z-sticky: 10; --cc-z-side: 20; --cc-z-pop: 100; --cc-z-dialog: 200; --cc-z-toast: 300; --cc-z-tip: 400;
  --cc-focus: var(--cc-acc); --cc-focus-w: 2px; --cc-focus-off: 2px;
}
:root {            /* 浅色 */                        html.dark {      /* 深色 */
  --cc-info: #1f5fbf; --cc-info-soft: #286edc14;     --cc-info: #7db4ff; --cc-info-soft: #7db4ff21;
  --cc-info-line: #1f5fbf66; --cc-info-text: #1a52a6; --cc-info-line: #7db4ff73; --cc-info-text: #9cc6ff;
  --cc-warn: #7a5c00; --cc-warn-soft: #e6b41429;     --cc-warn: #f2cf5b; --cc-warn-soft: #f2cf5b21;
  --cc-warn-line: #a07800; --cc-warn-text: #7a5c00;  --cc-warn-line: #f2cf5b73; --cc-warn-text: #f2cf5b;
  --cc-err-soft: #c4283d14; --cc-err-line: #c4283d66; --cc-err-soft: #ff7b8a1f; --cc-err-line: #ff7b8a73;
  --cc-err-text: #b01f33; --cc-ok-line: #00826666;   --cc-err-text: #ff9aa6; --cc-ok-line: #4be3d066;
  --cc-skel: #e0e8f2; --cc-skel-hi: #eef3fa;         --cc-skel: #16233a; --cc-skel-hi: #1f3150;
  --cc-sh-1: 0 1px 2px #1e3c6e1f; --cc-faint: #566884; --cc-acc: #a65600;  /* 深色：--cc-faint: #8396b8; --cc-sh-1: 0 1px 2px #00000066 */
}
/* sh-2 / sh-3 / scrim 完整值见 docs/design/source/v2.css */</pre>'''
    body = f'''{sec('语义色 · 深浅各一套（solid / soft / line / text）', sem_both + uses, tags('维度·对比度与可读性'))}
{sec('对比度实测（文字落在卡片底色上）', card('', ctbl + '<p class="sp-note">这张表由设计稿生成脚本按取值现算，不是目测。实现后用同一份算法写成 e2e 检查。</p>'), tags('维度·对比度与可读性'))}
<div class="grid2" style="grid-template-columns:1.2fr 1fr;align-items:start">{sec('新增令牌清单', card('', tt))}<div class="colf" style="gap:24px">{sec('值有变化的现有令牌', card('', ct))}{sec('可复制的 CSS 草稿（节选）', css)}</div></div>'''
    return sheet('设计系统 v2 · 色彩与令牌清单', '语义色补齐 soft / line / text 三个变体，焦点环和骨架屏进令牌。底色与琥珀强调沿用现有深蓝黑，不换色相。', body, audit=('维度·对比度与可读性', 19))

# ---------- 组件 1 ----------
def board_cmp1():
    kinds = [('pri', '主按钮', '一个区域只放一个'), ('sec', '次按钮', '常规操作'), ('txt', '文字按钮', '低强度、工具条里'), ('dng', '危险按钮', '删除 / 清除 / 取消任务'), ('ico', '图标按钮', '必须有提示和 aria-label')]
    states = [('默认', ''), ('悬停', 'hov'), ('按下', 'act'), ('聚焦', 'foc'), ('禁用', 'dis'), ('加载中', 'ld')]
    def cell(k, st):
        if k == 'ico': return btn('', 'ico bord', icon='more', st=st, label='更多')
        return btn('保存' if k != 'dng' else '清除', k, st=st, icon=('plus' if (k == 'pri' and st == 'foc') else None))
    head = '<tr><th></th>' + ''.join(f'<th>{n}</th>' for n, _ in states) + '</tr>'
    rows = ''.join(f'<tr><td style="white-space:nowrap"><b style="color:var(--cc-fg)">{n}</b><div class="xs">{u}</div></td>' + ''.join(f'<td>{cell(k, st)}</td>' for _, st in states) + '</tr>' for k, n, u in kinds)
    mat = f'<div style="overflow-x:auto"><table class="tbl"><thead>{head}</thead><tbody>{rows}</tbody></table></div>'
    szs = f'<div class="rowf" style="gap:16px;align-items:flex-end">' + ''.join(f'<div class="colf" style="gap:8px;align-items:flex-start">{btn("按钮", "pri", sz, "download")}{btn("按钮", "sec", sz)}{btn("", "ico bord", sz, "more", label="更多")}<span class="lbl">{n}</span></div>' for sz, n in [('s', 'S · 28px · 表格行、卡片内'), ('', 'M · 36px · 默认'), ('l', 'L · 44px · 首屏主操作')]) + '</div>'
    light = stage('light', f'<div class="rowf">{btn("保存", "pri")}{btn("保存", "pri", st="hov")}{btn("保存", "pri", st="dis")}{btn("保存", "pri", st="ld")}{btn("取消", "sec")}{btn("详情", "txt")}{btn("清除", "dng")}{btn("", "ico bord", icon="more", label="更多")}{btn("保存", "sec", st="foc")}</div>')
    dk = stage('dark', f'<div class="colf">{mat}</div>')
    rules = ''.join([rule('do', '一个视图（卡片 / 对话框 / 行）最多一个主按钮；其余降为次按钮或文字按钮。'), rule('do', '按钮文字是动词：“添加订阅”“重试”“检测模型”。不要“确定 / 提交”。'), rule('do', '禁用的按钮必须能看出原因：旁边一行说明，或悬停提示（见工具箱首页）。'), rule('do', '加载中保持按钮宽度不变，图标换成转圈，并忽略再次点击。'), rule('dont', '不用琥珀色文字当“可点”，行内动作要么是 CcButton 文字型，要么是 CcLink。'), rule('dont', '纯文字、没有悬停底色的“伪按钮”（现有 .pz-link / .lnk / .mi / .ab）。')])
    bt = f'{dk}{light}<div class="grid2" style="gap:24px"><div class="colf"><div class="lbl">尺寸</div>{szs}</div><div class="colf">{rules}</div></div>'
    mp = '<p class="sp-note">实现：<code>CcButton.vue</code>，props: <code>kind=primary|secondary|text|danger</code> · <code>size=s|m|l</code> · <code>icon</code> · <code>loading</code> · <code>disabledReason</code>。内部基于 el-button，用 --cc-* 覆盖。</p>'
    # segmented
    sg = both(f'<div class="colf" style="gap:16px"><div class="rowf">{seg(["自动平衡", "按顺序", "单列"], 0)}{seg(["跟随系统", "深色", "浅色"], 1)}</div><div class="rowf">{seg(["列表", "网格"], 1, "s")}{seg(["原页", "译文", "原页+译文"], 2, dis=(0,))}</div></div>')
    sgr = ''.join([rule('do', '2 到 4 项，每项 ≤ 6 个字；放不下就换 CcSelect。'), rule('do', '选中 = 卡片底 + 强调色字 + 1px 强调线；不用整块琥珀填充（那是主按钮的外观）。'), rule('do', '禁用项保留位置、文字变淡，并带提示原因。'), rule('dont', '页面切换用 tab，不用 segmented；segmented 只切“同一块内容的视图 / 模式”。'), rule('dont', '三套并存的实现（el-radio-button / 自绘 .seg / 自绘 .tabs）。')])
    # link
    lk = both(f'<div class="colf" style="gap:12px"><p style="font-size:12px;color:var(--cc-mute)">可以自建，或从 {link("公共术语库", ext=True)} 安装。{link("了解更多")}</p><div class="rowf">{link("默认")}{link("悬停", "hov")}{link("已访问", "vis")}{link("不可用", "dis")}</div></div>')
    lkr = ''.join([rule('do', '用在说明文字里的跳转：下划线 + 强调色文字（深 #ffc760 / 浅 #8a4a00）。'), rule('do', '会打开浏览器的链接末尾加外链图标。'), rule('dont', '浏览器默认蓝色下划线（设置 · AI 现状）。'), rule('dont', '用链接触发保存、删除这类动作，用 CcButton 文字型。')])
    # select
    se = both(f'''<div class="rowf" style="align-items:flex-start;gap:24px"><div class="colf" style="gap:8px;width:200px">{sel("通用", w=200)}{sel("请选择场景", w=200, ph=True)}{sel("通用", w=200, open_=True)}{menu([("通用", "check", "sel"), ("小红书", "", "hov"), ("微博",), ("朋友圈",), ("周报",)], 200)}</div><div class="colf" style="gap:8px;width:200px">{sel("听写：跟随设置", "s", 200)}{sel("已禁用", w=200).replace("c-sel", "c-sel dis")}<div class="c-sel" style="width:200px;border-color:var(--cc-err)"><span class="v">未选择</span>{i("chev", 14)}</div><div class="errm" style="display:flex;gap:4px;align-items:center;font-size:12px;color:var(--cc-err-text)">{i("alert", 14)}请选择一个模型</div></div></div>''')
    ser = ''.join([rule('do', '一律用 CcSelect，替换写作窗里的原生 &lt;select&gt;：高度 36 / 28，圆角 8，菜单 sh-2。'), rule('do', '超过 8 项出现搜索框；选中项左侧有勾。'), rule('do', '宽度跟着所在字段行：S 120 · M 180 · L 260 · 通栏。同一张卡里同类控件等宽对齐。'), rule('do', '键盘：Enter/空格展开，上下选择，Esc 关闭；焦点环见令牌。')])
    body = f'''{sec('CcButton', bt + mp, tags(5, 16, '维度·控件一致性'))}
{sec('CcSegmented', sg + f'<div class="grid2">{card("使用规则", sgr)}{card("替换", "<ul><li>3 套实现合一：<code>CcSegmented.vue</code>，v-model + options。</li><li>设置页排布、主题、取景方式、预览档位等 40+ 处全部走它。</li></ul>")}</div>', tags('维度·控件一致性'))}
<div class="grid2" style="align-items:start">{sec('CcLink', lk + card('使用规则', lkr), tags(1, 16))}{sec('CcSelect', se + card('使用规则', ser), tags(16))}</div>'''
    return sheet('组件规范 · 按钮 / 分段 / 链接 / 选择', '每个组件给出全部状态（深色为主、浅色对照）和使用规则。静态图里的悬停 / 按下 / 聚焦是强制显示出来的。', body, audit=(5, 16, '维度·控件一致性'))

# ---------- 组件 2 ----------
def board_cmp2():
    long_desc = '指定使用哪个专家。“通用”适合大多数场景；翻译字幕、网页时会按网站选择对应的专家，也可以在这里手动指定，译文格式始终由程序保证。'
    f1 = field('默认专家', '“通用”适合大多数场景。', sel('通用', w=180))
    f2 = field('自动选用专家', long_desc, sw(True, '自动选用'), more=True)
    f3 = field('模型接口地址', '以 https:// 开头，不要带结尾的 /v1。', inp('http://localhost:11434', w=260, st='err'), err='地址不能访问，检查端口是否正确（连接被拒绝）')
    f4 = field('术语库', '让专有名词保持同一个译法。', f'{btn("新建术语库", "sec", "s")}{btn("新建文件夹", "sec", "s")}{btn("导入 CSV…", "sec", "s")}', col=True)
    fb = both(f'<div class="colf" style="gap:0;width:100%">{f1}{f2}{f3}{f4}</div>')
    anat = f'''<div class="colf" style="gap:12px">{ann(1, "<strong>标签</strong>：14px / 500，一句话，是名词或“动词+宾语”。")}{ann(2, "<strong>说明</strong>：12px / --cc-mute，<strong>最多 2 行</strong>，超出自动省略；真正需要更长，放进“了解更多”。")}{ann(3, "<strong>控件</strong>：固定靠右，宽度 S/M/L 三档；控件很长（输入多行、一排按钮）用堆叠版 <code>col</code>，控件落在说明下方。")}{ann(4, "<strong>错误</strong>：控件边框变危险色 + 行下方一行图标 + 说明如何解决，不只说“无效”。")}{ann(5, "<strong>了解更多</strong>：点开在原位展开完整说明（<code>--cc-dur-base</code>），再点收起；不跳转、不弹窗。")}{ann(6, "<strong>行间</strong>：上下 12px，行与行用 1px <code>--cc-line-soft</code> 分隔；卡片内第一行没有分隔线。")}</div>'''
    expd = both(f'<div class="c-field" style="grid-template-columns:1fr auto"><div class="lab">自动选用专家</div><div class="ctl">{sw(True, "自动选用")}</div><div class="desc" style="-webkit-line-clamp:unset;display:block;grid-column:1 / -1;padding:8px 12px;border-left:2px solid var(--cc-line-strong);margin-top:4px">指定使用哪个专家。“通用”适合大多数场景；翻译字幕、网页时会按网站选择对应的专家，也可以在这里手动指定。译文格式始终由程序保证，专家的说明只会补充用词和语气。{link("收起")}</div></div>', '（展开后）')
    # empty
    es = both(f'<div class="colf" style="gap:12px;width:100%"><div class="pn" style="padding:8px">{empty("bell", "还没有订阅", "粘贴频道、UP 主或播放列表链接，有新内容时自动下载。", btn("添加订阅", "pri", icon="plus"))}</div><div class="colf" style="gap:12px">{empty("cpu", "模型池是空的", "添加一个有免费额度的服务就能开始。", btn("添加服务", "pri", "s"), cmp=True)}{empty("lib", "没有符合筛选的文件", "换个条件试试。", btn("清除筛选", "sec", "s"), cmp=True)}</div></div>')
    esr = ''.join([rule('do', '结构固定：图（64px 圆角块 + 线性图标）→ 标题（16px）→ 说明（12px，≤ 2 行）→ 0–2 个操作。'), rule('do', '有明确下一步才放主操作，按钮文字就是那个动作（“添加订阅”，不是“开启订阅”）。'), rule('do', '三种尺寸：整页（上下 40）、卡片内（上下 24）、紧凑行（左图右文，用于设置卡里）。'), rule('dont', '空的表格只剩表头；空状态必须替换整个列表区。'), rule('dont', '用插画或吉祥物：图标统一用线性图标，保持克制。')])
    # skeleton
    skl = both(f'''<div class="colf" style="gap:16px;width:100%"><div class="rowf" style="gap:12px;align-items:center;flex-wrap:nowrap">{sk("96px", 54, 6)}<div class="colf" style="gap:8px;flex:1">{sk("70%", 14)}{sk("100%", 4)}{sk("40%", 11)}</div>{sk("64px", 28, 8)}</div><div class="rowf" style="gap:12px;align-items:center;flex-wrap:nowrap">{sk("96px", 54, 6)}<div class="colf" style="gap:8px;flex:1">{sk("55%", 14)}{sk("100%", 4)}{sk("30%", 11)}</div>{sk("64px", 28, 8)}</div><div class="rowf" style="gap:8px">{sk("100%", 12)}{sk("92%", 12)}{sk("60%", 12)}</div></div>''')
    skr = ''.join([rule('do', '形状和真实内容的布局一致（缩略图 + 两行文字 + 进度 + 按钮），数量取一屏可见的行数（媒体库 8 张、队列 5 行）。'), rule('do', '0.3 秒内出结果就不显示骨架，避免闪烁；超过 10 秒转成带“重试”的错误。'), rule('do', '光带 1.4s 循环；<code>prefers-reduced-motion</code> 下改为静态底色。'), rule('dont', '整页转圈遮罩（现有 v-loading 13 处）。')])
    # dialog
    dg1 = f'''<div class="c-dlg" style="width:400px"><header>{i("alert", 18)}<h4>取消这个下载？</h4></header><div class="bd"><span>“【4K 延时】上海 · 夜未央”已下载 28.6 MB，取消后会保留已下载的部分，可以稍后继续。</span></div><footer>{btn("先不取消", "sec")}{btn("取消下载", "dng")}</footer></div>'''
    dg2 = f'''<div class="c-dlg" style="width:560px"><header><h4>添加服务</h4><span class="sp" style="flex:1"></span>{btn("", "ico s", icon="x", label="关闭")}</header><div class="bd" style="gap:0">{field("服务商", "", sel("OpenAI 兼容", w=220))}{field("接口地址", "", inp("https://api.example.com", w=260))}{field("密钥", "保存在本机，不进设置文件和备份。", inp("••••••••", w=260))}</div><footer>{btn("取消", "sec")}{btn("检测并保存", "pri")}</footer></div>'''
    dgw = table(['规格', '宽度', '用于', '窄屏规则'], [[code('sm'), '400px', '确认 / 危险操作，≤ 3 行内容', 'width: min(400px, 100vw − 32px)'], [code('md'), '560px', '表单、添加服务、重命名', '同上；底部按钮 < 480 时改为纵向通栏'], [code('lg'), '720px', '列表选择、预览、多栏设置', '同上；高度 ≤ 85vh，正文区内部滚动，页头页脚固定'], [code('full'), '100% − 48px', '播放器、字幕校对（现有 PlayerDialog）', '<900 时全屏，无圆角']])
    dgr = ''.join([rule('do', '22 个 el-dialog 统一页头（标题 16px/700 + 右上关闭）与页脚（右对齐：次 → 主，页脚底色 --cc-foot）。'), rule('do', '危险确认：标题说清后果，正文说“会发生什么 / 能否恢复”，主按钮用危险型，文字是动作本身，焦点默认落在“先不…”上。'), rule('do', '遮罩 --cc-scrim，z-index 200；Esc 关闭，焦点锁在对话框内，关闭后回到触发处。'), rule('dont', '在对话框里再开对话框；确认后的结果用 Toast。')])
    # toast
    tt = both(f'<div class="colf" style="gap:12px;align-items:flex-start">{toast("err", "字幕下载失败", "连接被重置（HTTP 403）。可能需要登录或更换节点。", link("重试") + link("查看详情"))}{toast("ok", "已加入下载队列")}{toast("warn", "磁盘剩余空间不足 2 GB", "继续下载可能失败。", link("打开设置"))}{toast("info", "已复制链接")}</div>')
    ttr = table(['类型', '停留', '操作'], [['成功 / 信息', '3 秒自动消失', '可带 1 个撤销'], ['警告', '6 秒，悬停暂停', '可带 1 个跳转'], ['错误', '<b>不自动消失</b>，直到关闭；悬停无关', '<b>重试 + 查看详情</b>；详情展开原始错误、可复制']])
    ttrules = ''.join([rule('do', '右下堆叠，最多同时 3 条，新的在下；z-index 300；读屏 role=status（错误用 alert）。'), rule('do', '标题 14px 说发生了什么，说明 12px 说为什么与怎么办，不写“失败了，请重试”。'), rule('dont', '用 el-message 的单行瞬时提示报错（现状，错误一闪而过看不清）。')])
    body = f'''{sec('CcField · 标准设置行', fb + f'<div class="grid2">{card("结构与规则", anat)}<div class="colf">{expd}<p class="sp-note">说明超过 2 行时的做法：折叠，“了解更多”在原位展开。上方第二行就是折叠态，这里是展开态。</p></div></div>', tags(1, 2, '维度·文案一致性'))}
<div class="grid2" style="align-items:start">{sec('CcEmpty', es + card('使用规则', esr), tags(9, 11, 17))}{sec('CcSkeleton', skl + card('使用规则', skr), tags('维度·空/加载/错误状态'))}</div>
{sec('CcDialog · 宽度自适应', f'<div class="scrim" style="gap:24px;flex-wrap:wrap;align-items:flex-start;background:var(--cc-scrim)">{dg1}{dg2}</div><div class="grid2" style="align-items:start">{card("宽度规则", dgw)}{card("使用规则", dgr)}</div>', tags('维度·控件一致性'))}
{sec('CcToast · 带重试与详情的错误', tt + f'<div class="grid2" style="align-items:start">{card("停留与操作", ttr)}{card("使用规则", ttrules)}</div>', tags('维度·空/加载/错误状态'))}'''
    return sheet('组件规范 · 字段 / 空状态 / 骨架 / 对话框 / 提示', 'CcField 是设置页的骨架：151 处手写的“标签 + 说明 + 控件”收成一个组件。', body, audit=(1, 9, 11, '维度·空/加载/错误状态'))

# ---------- C11 加载·错误·反馈 ----------
def board_c11():
    sk_list = both(f'''<div class="colf" style="width:100%;gap:12px"><div class="lbl">队列加载中</div>''' + ''.join(f'<div class="rowf" style="gap:12px;flex-wrap:nowrap;align-items:center">{sk("96px", 54, 6)}<div class="colf" style="gap:8px;flex:1">{sk(str(70 - 8 * n) + "%", 14)}{sk("100%", 4)}</div>{sk("72px", 28, 8)}</div>' for n in range(3)) + '</div>')
    sk_grid = both(f'<div class="colf" style="width:100%;gap:12px"><div class="lbl">媒体库加载中</div><div class="grid4" style="grid-template-columns:repeat(4,1fr)">' + ''.join(f'<div class="colf" style="gap:8px">{sk("100%", 62, 6)}{sk("80%", 12)}{sk("50%", 10)}</div>' for _ in range(4)) + '</div></div>')
    # list motion strip
    def frame(label, body, sub):
        return f'<div class="colf" style="gap:8px;flex:1;min-width:0"><div class="lbl">{label}</div><div class="colf" style="gap:6px">{body}</div><div class="sm">{sub}</div></div>'
    row_ = lambda t, st='': f'<div style="display:flex;align-items:center;gap:8px;padding:8px 12px;border:1px solid var(--cc-line);border-radius:8px;background:var(--cc-card);font-size:12px;{st}">{thumb("t2", w=40)}<span class="ell">{t}</span></div>'
    motion = f'''<div class="rowf" style="gap:20px;align-items:flex-start;flex-wrap:nowrap">{frame("新增 · 200ms ease-out", row_("新任务 · 夜未央", "opacity:.35;transform:translateY(-8px);border-style:dashed") + row_("示例视频 1") + row_("示例视频 2"), "新行从上方 8px 淡入落位，下面的行同时顺延。")}{frame("移除 · 200ms ease-in", row_("示例视频 1") + row_("示例视频 2", "opacity:.3;transform:scaleY(.6)") + row_("示例视频 3"), "先淡出，再收起高度，下面的行补位。")}{frame("状态变化 · 120ms", row_("示例视频 1", "box-shadow:0 0 0 1px var(--cc-ok-line);background:var(--cc-ok-soft)") + row_("示例视频 2") + row_("示例视频 3"), "完成时整行底色短暂变成成功色再退回，不弹跳。")}</div>'''
    page_t = f'''<div class="rowf" style="gap:24px;flex-wrap:nowrap;align-items:center"><div style="display:grid;grid-template-columns:44px 1fr;width:260px;height:130px;border:1px solid var(--cc-line-strong);border-radius:8px;overflow:hidden;background:var(--cc-bg)"><div style="background:var(--cc-side);border-right:1px solid var(--cc-line);display:flex;flex-direction:column;gap:8px;padding:12px 10px">{"".join(f'<i style="display:block;height:16px;border-radius:4px;background:{"var(--cc-acc)" if n == 1 else "var(--cc-line)"}"></i>' for n in range(4))}</div><div style="padding:12px;display:flex;flex-direction:column;gap:8px"><i style="display:block;height:14px;width:50%;border-radius:4px;background:var(--cc-fg2)"></i><i style="display:block;height:48px;border-radius:8px;background:var(--cc-card);border:1px solid var(--cc-line)"></i><i style="display:block;height:48px;border-radius:8px;background:var(--cc-card);border:1px solid var(--cc-line);opacity:.5"></i></div></div><ul style="font-size:12px;color:var(--cc-mute);padding-left:16px;display:flex;flex-direction:column;gap:4px"><li><b style="color:var(--cc-fg)">侧栏不动</b>，只切换内容区。</li><li>新内容 <code>opacity 0→1</code> + <code>translateY(4px→0)</code>，<code>--cc-dur-slow</code> 320ms，<code>--cc-ease-out</code>。</li><li>旧内容不做退出动画（直接替换），避免两页叠在一起。</li><li>数据没到时先显示骨架，不显示空状态。</li></ul></div>'''
    err_page = both(f'<div class="pn" style="width:100%;padding:8px">{empty("alert", "加载媒体库失败", "读取数据库时出错：database is locked。通常是另一个清影窗口正在写入。", btn("重试", "pri") + btn("查看详情", "txt"))}</div>', '')
    inline = both(f'<div class="colf" style="gap:12px;width:100%">{callout("err", "<b>Cookie 已过期</b>，B 站账号需要重新登录才能下载 1080P 以上。")}<div class="c-field" style="padding:0"><div class="lab">模型接口地址</div><div class="ctl">{inp("http://localhost:11434", w=240, st="err")}</div><div class="errm">{i("alert", 14)}连接被拒绝，检查端口是否正确</div></div></div>')
    rm = table(['动效', '默认', 'prefers-reduced-motion: reduce'], [['页面切换', '淡入 + 上移 4px，320ms', '只淡入，100ms，无位移'], ['列表行进出', '淡入/收起，200ms', '直接出现 / 消失'], ['骨架光带', '1.4s 循环', '静态底色，不流动'], ['转圈（加载中）', '0.8s 旋转', '保留旋转（它传达“还在进行”）；或换成“加载中…”文字'], ['开关、展开、菜单', '200ms 位移/淡入', '去掉位移，只保留颜色变化'], ['完成高亮', '底色闪一下', '底色保持 1 秒后退回，无过渡']])
    rmcss = '<pre style="margin:0;padding:12px;border-radius:8px;background:var(--cc-fill);border:1px solid var(--cc-line);font-family:var(--cc-mono);font-size:11px;line-height:1.6;color:var(--cc-fg2)">@media (prefers-reduced-motion: reduce) {\n  :root { --cc-dur-fast: 0.01ms; --cc-dur-base: 0.01ms; --cc-dur-slow: 100ms; }\n  .cc-skeleton { animation: none; }\n}</pre>'
    body = f'''{sec('骨架屏 · 取代整页转圈', f'<div class="grid2" style="align-items:start">{sk_list}{sk_grid}</div>', tags('维度·空/加载/错误状态'))}
{sec('列表进出动画', card('', motion), tags('维度·动效与反馈'))}
{sec('页面切换', card('', page_t), tags('维度·动效与反馈'))}
<div class="grid2" style="align-items:start">{sec('页面级错误', err_page, tags('维度·空/加载/错误状态'))}{sec('行内错误', inline)}</div>
{sec('错误 Toast（带重试 / 详情）', both(f'<div class="colf" style="gap:12px;align-items:flex-start;width:100%">{toast("err", "下载失败：示例视频 4", "连接被重置（HTTP 403）。", link("重试") + link("查看详情"))}{toast("err", "无法保存设置", "写入 settings.json 被拒绝：磁盘只读。", link("重试") + link("复制详情"))}</div>'), tags('维度·空/加载/错误状态'))}
<div class="grid2" style="align-items:start">{sec('减弱动效说明', card('', rm + rmcss), tags('维度·动效与反馈'))}{sec('保存反馈（与自动保存一致）', card('', f'<div class="colf" style="gap:12px"><div class="rowf">{chip("正在保存…", "info", True)}{chip("已自动保存", "ok", True)}{chip("保存失败 · 重试", "err", True)}</div><ul><li>顶部状态胶囊三态：保存中 → 已自动保存（3 秒后淡成灰字）→ 失败（常驻，带重试）。</li><li>删掉所有“改完点保存才生效”文案。需要保存按钮的编辑（新建服务、对话框）按钮文字是动作，不叫“保存设置”。</li></ul></div>'), tags(1))}</div>'''
    return sheet('加载 · 错误 · 反馈', '没有骨架、没有进出动画、错误一闪而过：这三件事决定“用起来顺不顺”。全部走动效令牌，并给出减弱动效的对应写法。', body, audit=('维度·空/加载/错误状态', '维度·动效与反馈'))

# ---------- C12 硬编码色 ----------
def board_c12():
    rows = [
        ['TransferPanel.vue（手机互传）', '#ff7a14 #ff8c33 #e86a08 #1a1208 #fff', '橙色按钮/徽标、深色字、白字', code('--cc-acc-strong') + ' ' + code('--cc-acc-strong-hover') + ' ' + code('--cc-on-acc'), '橙色并入琥珀，与全局主按钮一致'],
        ['WriteApp.vue（写作窗）', '#b45f00 #ffb547 #ffc760 #d3deec #2c4366 #1a1100 #fff', '重复写了一遍浅/深两套琥珀与线色', code('--cc-acc') + ' ' + code('--cc-acc-text') + ' ' + code('--cc-line') + ' ' + code('--cc-line-strong') + ' ' + code('--cc-on-acc'), '本就是令牌值的硬抄，直接换名'],
        ['ShotApp.vue（截图）', '#fff #0b1424 #eee #111 #c4283d #ffb547', '工具条、标注、危险色', code('--cc-fg') + ' ' + code('--cc-card') + ' ' + code('--cc-err') + ' ' + code('--cc-acc'), '截图浮层固定深色：在其作用域内重定义令牌为深色'],
        ['FloatApp.vue（悬浮窗）', '#ffb547 #e8f0fd #9aabc6 #4be3d0 #ff7b8a #d3deec', '深色胶囊，不随主题', '同名 --cc-* 深色值', '设计决定：保持固定深色（审计 #18 认为可接受），但取值来自令牌，不再手抄'],
        ['ParseView.vue', '#fff #000000a6 #ffffffe6 #ffffff99 #ffffff80', '缩略图上的文字与蒙层', code('--cc-on-media') + ' ' + code('--cc-scrim-media') + '（NEW）', '缩略图上永远是黑蒙层 + 白字，两个主题一样，单独起名，不要借用 --cc-on-acc'],
        ['ToolPreview.vue', '#fdf0e6 #f3cfb2 #f0b27f #e6f6ec #b4e2c6 #a14a12', '预览示意用的暖/绿块', code('--cc-acc-soft') + ' ' + code('--cc-ok-soft') + ' ' + code('--cc-acc-text'), '示意图而已，用 soft + 文字令牌，深色下不再发亮'],
        ['PdfCanvas.vue', '#fff #ccc #d9762b #b4561a #2f8a57 #b7beb9', 'PDF 纸张与批注色', '纸张 ' + code('#fff') + ' 保留（它就是白纸）；批注 ' + code('--cc-acc') + ' ' + code('--cc-ok'), '白纸是内容不是 UI，写成 ' + code('--cc-paper') + '（NEW，两主题同为白）'],
        ['ColorPicker.vue', '#f00 #ff0 #f0f #0ff #0f0 #fff #1d2433', '取色色相环与界面', '色相环色值是内容，保留；界面部分 ' + code('--cc-card') + ' ' + code('--cc-fg'), '只清理界面色，不动色相环'],
        ['LibraryView.vue（媒体库）', '#f5b73b #e6a23c #fff', '收藏星、丢失标记、白字', code('--cc-warn') + ' ' + code('--cc-warn') + ' ' + code('--cc-on-acc'), '浅色下 #f5b73b 白底对比度仅 1.9，换 warn 后 6.1'],
        ['EditTimeline.vue / EditMonitor.vue', '#e5484d #d9a21a #b9812d #b4561a #4f7cc9 …', '时间线轨道/片段色', '轨道色板 ' + code('--cc-track-1…6') + '（NEW）', '剪辑轨道要区分种类，单独一组 6 色令牌；仍放浅深两套'],
    ]
    t = table(['文件', '现有硬编码', '是什么', '改用令牌', '说明'], rows)
    rule1 = '''<pre style="margin:0;padding:12px;border-radius:8px;background:var(--cc-fill);border:1px solid var(--cc-line);font-family:var(--cc-mono);font-size:11px;line-height:1.6;color:var(--cc-fg2)">// stylelint：.vue 的 &lt;style&gt; 里禁止 hex / rgb(a) / hsl(a) 字面量
"color-no-hex": true,
"declaration-property-value-disallowed-list": { "/^(color|background|border|fill|stroke|box-shadow)/": ["/rgba?\\(/", "/hsla?\\(/"] }
// 例外只允许 theme.css、skin.css 与白名单（色相环、PDF 纸张）</pre>'''
    body = f'''{sec('哪些文件的哪些色，改用什么令牌', card('', t + '<p class="sp-note">数字来自仓库现状的字面量统计（本稿 grep 结果，和审计报告的计数口径略有出入）。表中 NEW 的令牌是为“内容色”和“媒体上的文字”新增的少数几个，其余全部映射到已有或本稿新增的语义令牌。</p>'), tags(20))}
<div class="grid2" style="align-items:start">{sec('防回退：lint 规则', rule1)}{sec('原则', card('', "".join([rule('do', '界面色全部走 --cc-*；内容色（色相环、PDF 纸张、缩略图蒙层）单独命名，不借用语义令牌。'), rule('do', '悬浮窗、截图浮层这类“固定深色”窗口：在窗口根元素重定义令牌，不在每条规则里写色。'), rule('dont', '直接复制 theme.css 里的 hex 到组件里（WriteApp 现状）：改一次主题要改两处。')])))}</div>'''
    return sheet('硬编码色清理对照表', '审计 #20：约 200 处硬编码色不会随主题变化。这张表说明每个文件里的哪些色用哪个令牌，以及怎样防止再长回来。', body, audit=(19, 20))

# ---------- D 实施清单 ----------
def board_d():
    R = 'desktop/src/'
    rows = [
        ['1', '令牌', '字号 / 圆角 / 间距 / 阴影 / 动效 / z-index / 焦点 / 语义色；改 faint 与浅色 acc', code(R + 'styles/theme.css'), '0.5 天', '无', '新令牌全部有浅深两值；旧值无引用破坏（保留 --cc-shadow 别名）'],
        ['2', '全局规则', '焦点环全局规则；reduced-motion；el-* 变量映射新令牌；字号/圆角批量替换', code(R + 'styles/skin.css'), '1.5 天', '1', '全局检索 `font-size: *.5px`、旧圆角为 0'],
        ['3', 'CcButton', '5 类型 × 3 尺寸 × 状态；替换自绘 .pz-link/.lnk/.mi/.ab', code(R + 'components/ui/CcButton.vue'), '1 天', '1,2', '状态图与本稿一致；仅 1 个主按钮/卡片'],
        ['4', 'CcSegmented / CcSelect / CcLink', '合并 3 套 segmented；替换原生 select；统一链接', code(R + 'components/ui/CcSegmented.vue CcSelect.vue CcLink.vue'), '1.5 天', '3', '写作窗与设置页无原生 select；无默认蓝链接'],
        ['5', 'CcField / CcSwitch', '标签 + 说明（≤2 行）+ 控件 + 错误；“了解更多”折叠', code(R + 'components/ui/CcField.vue'), '1.5 天', '3,4', '设置页 151 处手写行替换为组件；说明超 2 行自动折叠'],
        ['6', 'CcEmpty / CcSkeleton', '三种尺寸；骨架（行/卡片/文本）', code(R + 'components/ui/CcEmpty.vue CcSkeleton.vue'), '1 天', '3', '空表只剩表头的页面为 0'],
        ['7', 'CcDialog / CcToast', '统一 22 个 el-dialog 的页头页脚；错误 toast 带重试 / 详情', code(R + 'components/ui/CcDialog.vue CcToast.vue；utils/notify.ts'), '2 天', '3,6', '错误不自动消失；Esc 关闭；焦点回到触发处'],
        ['8', 'C7 侧栏', '矮窗口滚动 + 渐隐、仅图标模式、页脚短文案、队列徽标', code(R + 'App.vue (.side)'), '1 天', '2,3', '900×700 logo 与设置入口始终可见'],
        ['9', 'C2 + C1 + C4 设置', '去掉分类页的“排布”开关，移到通用；AI 页折叠说明；安全页快捷键控件', code(R + 'views/SettingsView.vue components/AiPanel.vue AiPool.vue GlossaryPanel.vue ExpertsPanel.vue SecurityPanel.vue GeneralPanel（通用）'), '3 天', '5,4', '各分类页右上无排布开关；老板键为录入控件'],
        ['10', 'C3 下载队列', '主操作图标按钮 + 更多菜单；底部渐隐；统计合并为一条', code(R + 'views/QueueView.vue'), '2 天', '3,4,6', '最后一行不被底栏裁切；900 宽无折行空白'],
        ['11', 'C6 工具箱首页', '高频快捷卡；三列对齐；禁用原因', code(R + 'views/ToolboxView.vue components/toolbox/*'), '2 天', '3,5', '“开始处理”禁用时有原因说明'],
        ['12', 'C8 媒体库', '筛选弹层、空/加载态、清理硬编码色', code(R + 'views/LibraryView.vue components/library/*'), '2 天', '4,6', '加载态是骨架；空态有主操作'],
        ['13', 'C10 屏幕翻译', '“选择目标”主按钮；模式卡紧凑；选项折叠', code(R + 'views/ScreenView.vue'), '2 天', '3,4,5', '首屏无需滚动即可开始翻译'],
        ['14', 'C5 写作窗', 'CcSelect；两层 tab 合一；清理硬编码色', code(R + 'WriteApp.vue'), '1.5 天', '3,4', '无原生 select；一层 tab'],
        ['15', 'C9 空状态统一', '订阅 / 生词 / 迷你窗 / 直播 / 文件翻译', code(R + 'views/SubsView.vue VocabView.vue LiveView.vue FileTransView.vue MiniApp.vue'), '1 天', '6', '均用 CcEmpty；订阅按钮叫“添加订阅”'],
        ['16', 'C11 动效与反馈', '页面切换、列表进出动画、保存状态胶囊', code(R + 'App.vue + 各列表组件'), '1.5 天', '1,2,6', 'reduced-motion 下无位移'],
        ['17', 'C12 硬编码色', 'TransferPanel / FloatApp / ShotApp / ToolPreview / ParseView / PdfCanvas / ColorPicker', code(R + '对应文件；.stylelintrc'), '2 天', '1', 'lint 规则通过，浅/深截图无色块残留'],
        ['18', '视觉回归基线', '浅/深 × 1280/900 的 Playwright 截图；对比度脚本；溢出检查', code('desktop/e2e/ui-baseline.cjs（新）；e2e/run.mjs'), '1.5 天', '2', '加进默认套件，CI 跑'],
    ]
    t = table(['序', '工作项', '内容', '文件', '预计', '依赖', '完成标志'], rows)
    acc = [('截图对照', '浅 / 深 × 1280×800 / 900×700 四张基线图，与本稿对应画板逐页肉眼对照；PR 里贴新旧对比。差异 > 2% 的区域必须有说明。'),
           ('对比度 ≥ 4.5:1', '脚本遍历 --cc-fg / mute / faint / acc-text / ok-text / err-text / info-text / warn-text 对 card、bg 的组合；< 4.5 红灯。大字（≥ 20px）≥ 3:1。'),
           ('焦点环可见', 'Tab 走一遍每个页面，每个可聚焦元素出现 2px 强调色环（对相邻底色 ≥ 3:1）；鼠标点击不出现。e2e 里对 focus-visible 取样断言。'),
           ('窄屏无溢出', '900×700 下每个页面 document.scrollWidth ≤ clientWidth；侧栏可滚动，logo 与设置入口始终可见；设置、队列、屏幕翻译不出现被截断的按钮。'),
           ('动效', 'reduced-motion 下无位移动画；列表进出、页面切换时长取令牌，不再有裸的 transition: 0.3s。'),
           ('清理指标', '字号 .5 值 0 处；圆角只出现令牌值；.vue 内 hex 字面量仅剩白名单；原生 select 0 处。')]
    acct = ''.join(f'<div class="rule" style="grid-template-columns:130px 1fr"><b style="color:var(--cc-acc-text)">{a}</b><span>{b}</span></div>' for a, b in acc)
    order = '<div class="rowf" style="gap:8px;flex-wrap:nowrap;font-size:12px">' + ''.join(f'<span class="tag{" new" if k == 2 else ""}" style="height:28px;font-size:12px">{x}</span>' + (i('chevr', 14) if k < 3 else '') for k, x in enumerate(['① 令牌（1–2）', '② 组件（3–7）', '③ 页面（8–17）', '④ 基线与 lint（18）'])) + '</div>'
    total = '<p class="sp-note">合计约 <b>28 个工作日</b>（单人）。令牌与组件（约 9 天）完成后，页面改造互相独立，可以并行，每页一个 PR，各自带前后截图。设计稿确认前不动代码。</p>'
    body = f'''{sec('依赖顺序', order)}{sec('实施清单', card('', t + total))}{sec('验收标准', card('', acct))}'''
    return sheet('实施清单与验收', '先令牌 → 组件 → 页面。每个条目对应仓库里的文件，工作量按单人估，含自测与截图。', body, audit=())
