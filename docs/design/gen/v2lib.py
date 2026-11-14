# 设计系统 v2 画板的小部件。与 lib.py（旧画板）并存，不改旧画板。
import itertools
from lib import I as I0, ic

_n = itertools.count(1)
def uid(p='x'): return f'{p}{next(_n)}'

I = dict(I0)
I.update({
 'chev': 'M6 9l6 6 6-6', 'chevr': 'M9 6l6 6-6 6', 'chevu': 'M6 15l6-6 6 6', 'back': 'M15 6l-6 6 6 6',
 'info': 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM12 11v5M12 8h.01',
 'warn': 'M12 4 3 20h18L12 4ZM12 10v4M12 17h.01',
 'alert': 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM12 8v5M12 16h.01',
 'ext': 'M14 4h6v6M20 4l-9 9M18 14v5H5V6h5',
 'filter': 'M4 5h16l-6 8v6l-4-2v-4z',
 'camera': 'M4 8h4l2-3h4l2 3h4v11H4zM12 11a3 3 0 1 0 0 6 3 3 0 0 0 0-6',
 'film': 'M4 4h16v16H4zM8 4v16M16 4v16M4 9h4M4 15h4M16 9h4M16 15h4',
 'gif': 'M4 6h16v12H4zM9 10v4M11.500 10v4M15 12h2M15 10h3M15 10v4',
 'zip': 'M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5',
 'key': 'M8 15a4 4 0 1 1 3.500-6H21v3h-3v3h-3v-3h-3.500A4 4 0 0 1 8 15Z',
 'kbd': 'M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8',
 'up': 'M12 19V6M6 12l6-6 6 6',
 'eye': 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12ZM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6',
 'dots': 'M5 12h.01M12 12h.01M19 12h.01',
 'sidebar': 'M4 5h16v14H4zM9 5v14',
 'sun': 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 3v2M12 19v2M3 12h2M19 12h2M5.600 5.600 7 7M17 17l1.400 1.400M5.600 18.400 7 17M17 7l1.400-1.400',
 'moon': 'M20 14A8 8 0 0 1 10 4a8 8 0 1 0 10 10Z',
 'mini': 'M4 4h16v16H4zM12 12h8v8h-8z',
 'copy': 'M8 8h12v12H8zM4 16V4h12',
 'download': 'M12 4v11M7 11l5 5 5-5M5 20h14',
 'wand': 'M5 19 15 9M14 4l1 2 2 1-2 1-1 2-1-2-2-1 2-1zM19 12l.7 1.300 1.300.7-1.300.7L19 16l-.7-1.300L17 14l1.300-.7z',
 'drop': 'M12 3s6 6.500 6 11a6 6 0 0 1-12 0c0-4.500 6-11 6-11Z',
 'ok': 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM8 12l3 3 5-6',
})
def i(n, s=16, sw=1.7): return ic(I[n], s, sw)

# ---------- 组件 ----------
def btn(t='', kind='pri', size='', icon=None, st='', label=None, trail=None, extra=''):
    """kind: pri sec txt dng ico；st: hov act foc dis ld"""
    only = kind == 'ico'
    cls = f'c-btn {kind} {size} {st}'.strip()
    isz = 14 if size == 's' else (18 if size == 'l' else 16)
    ico = i(icon, isz) if icon else ''
    if 'ld' in st.split(): ico = '<span class="c-spin"></span>'
    tr = i(trail, 14) if trail else ''
    dis = ' disabled' if 'dis' in st.split() else ''
    al = f' aria-label="{label or t}"' if only else ''
    return f'<button class="{cls}"{al}{dis} {extra}>{ico}{"" if only else t}{tr}</button>'

def link(t, st='', ext=False):
    return f'<a class="c-link {st}" href="#">{t}{(" " + i("ext", 12)) if ext else ""}</a>'

def seg(opts, sel=0, size='', dis=()):
    b = ''.join(f'<button aria-pressed="{"true" if k == sel else "false"}"{" class=dis" if k in dis else ""}>{o}</button>' for k, o in enumerate(opts))
    return f'<span class="c-seg {size}" role="group">{b}</span>'

def sel(v, size='', w=None, open_=False, ph=False):
    st = f' style="width:{w}px;min-width:0"' if w else ''
    vv = f'<span class="v ell{" xs" if ph else ""}"{" style=color:var(--cc-faint)" if ph else ""}>{v}</span>'
    return f'<span class="c-sel {size}{" open" if open_ else ""}"{st} role="combobox" aria-label="选择">{vv}{i("chev", 14)}</span>'

def inp(v='', ph='', icon=None, w=None, st='', size='', sfx=''):
    sty = f' style="width:{w}px"' if w else ''
    body = f'<span>{v}</span>' if v else f'<span class="ph">{ph}</span>'
    return f'<span class="c-input {size} {st}"{sty}>{i(icon, 14) if icon else ""}{body}{("<span class=sfx>" + sfx + "</span>") if sfx else ""}</span>'

def sw(on=True, lab='开关'):
    return f'<button class="c-sw{" on" if on else ""}" role="switch" aria-checked="{"true" if on else "false"}" aria-label="{lab}"></button>'

def chip(t, k='', dot=False):
    return f'<span class="c-chip {k}">{"<i></i>" if dot else ""}{t}</span>'

def kbd(*keys): return ''.join(f'<span class="c-kbd">{k}</span>' for k in keys)

def field(label, desc='', ctl='', more=None, err=None, col=False, ico_label=None):
    d = f'<div class="desc">{desc}</div>' if desc else ''
    m = f'<span class="more">{link("了解更多")}</span>' if more else ''
    e = f'<div class="errm">{i("alert", 14)}{err}</div>' if err else ''
    return f'<div class="c-field{" col" if col else ""}"><div class="lab">{label}</div><div class="ctl">{ctl}</div>{d}{m}{e}</div>'

def empty(icon, title, desc, act='', cmp=False):
    a = f'<div class="act">{act}</div>' if act else ''
    if cmp:
        return f'<div class="c-empty cmp"><span class="art">{i(icon, 20)}</span><div class="tx"><h4>{title}</h4><p>{desc}</p></div>{a}</div>'
    return f'<div class="c-empty"><span class="art">{i(icon, 28, 1.5)}</span><h4>{title}</h4><p>{desc}</p>{a}</div>'

def toast(kind, title, desc='', actions=''):
    ic_ = {'err': 'alert', 'ok': 'ok', 'warn': 'warn', 'info': 'info'}[kind]
    d = f'<div class="d">{desc}</div>' if desc else ''
    a = f'<div class="a">{actions}</div>' if actions else ''
    return f'<div class="c-toast {kind}" role="status">{i(ic_, 18)}<div class="t">{title}</div>{i("x", 14)}{d}{a}</div>'

def callout(kind, text):
    ic_ = {'err': 'alert', 'ok': 'ok', 'warn': 'warn', 'info': 'info'}[kind]
    return f'<div class="c-callout {kind}">{i(ic_, 16)}<div>{text}</div></div>'

def menu(items, w=None):
    out = ''
    for it in items:
        if it == '-': out += '<hr>'; continue
        t, ic_, cl = (it + ('', ''))[:3] if isinstance(it, tuple) else (it, '', '')
        out += f'<div class="{cl}">{i(ic_, 14) if ic_ else ""}<span>{t}</span></div>'
    return f'<div class="c-menu"' + (f' style="width:{w}px"' if w else '') + f'>{out}</div>'

def sk(w='100%', h=12, r=None, extra=''):
    return f'<span class="c-sk" style="width:{w};height:{h}px;{("border-radius:" + str(r) + "px;") if r else ""}{extra}"></span>'

def prog(p, k=''): return f'<div class="prog {k}"><i style="width:{p}%"></i></div>'
def thumb(t='', dur='', w=None):
    d = f'<span class="dur">{dur}</span>' if dur else ''
    st = f' style="width:{w}px"' if w else ''
    return f'<div class="thumb {t}"{st}>{d}</div>'

def tag(t, k=''): return f'<span class="tag {k}">{t}</span>'
def _a(n): return f'审计 {n}' if isinstance(n, str) else f'审计 #{n}'
def tags(*nums):  # 审计编号
    return ''.join(tag(_a(n)) for n in nums)

# ---------- 说明页部件 ----------
def sheet(title, sub, body, audit=(), width=None):
    a = ''.join(tag(_a(n)) for n in audit)
    return f'''<div class="v2 sheet" data-t="dark"><div class="sh-head"><h1>{title}</h1>{a}<span class="sp"></span></div><p class="sub" style="color:var(--cc-mute);max-width:900px;margin-top:-16px">{sub}</p>{body}</div>'''

def sec(title, body, tg=''):
    return f'<section class="sect"><h2>{title}{tg}</h2>{body}</section>'

def card(title, body, extra='', cls=''):
    return f'<div class="sp-card {cls}"><h3>{title}{extra}</h3>{body}</div>'

def stage(theme, body, label=None, cls=''):
    lb = label or ('深色' if theme == 'dark' else '浅色')
    return f'<div class="stage {cls}" data-t="{theme}"><div class="stage-h">{lb}</div>{body}</div>'

def both(body, label=''):
    """同一块内容画深色 + 浅色两份"""
    return f'<div class="pair">{stage("dark", body, "深色" + label)}{stage("light", body, "浅色" + label)}</div>'

def rule(kind, t):
    return f'<div class="rule"><span class="{kind}">{i("check" if kind == "do" else "x", 14, 2)}</span><span>{t}</span></div>'

def ann(n, t): return f'<div class="ann"><b>{n}</b><span>{t}</span></div>'

def table(head, rows, cls=''):
    h = ''.join(f'<th>{x}</th>' for x in head)
    r = ''.join('<tr>' + ''.join(f'<td>{c}</td>' for c in row) + '</tr>' for row in rows)
    return f'<div style="overflow-x:auto"><table class="tbl {cls}"><thead><tr>{h}</tr></thead><tbody>{r}</tbody></table></div>'

def code(t): return f'<code>{t}</code>'

# ---------- 应用窗口 ----------
NAVS = [('parse', '解析', 'link'), ('queue', '下载队列', 'dl'), ('subs', '订阅', 'bell'), ('live', '直播', 'live'), ('screen', '屏幕翻译', 'screen'), ('library', '媒体库', 'lib'), ('filetr', '文件翻译', 'file'), ('vocab', '生词与摘录', 'vocab'), ('tools', '工具箱', 'tools')]
LOGO = '<svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 3l3.5 6.1M20.2 8.2l-6.8 1M20.2 15.8 15 11M12 21l-3.5-6.1M3.8 15.8l6.8-1M3.8 8.2 9 13"/></svg>'

def side(active, mode='full', badge=3, fade=False, hl=False):
    """mode: full 图标+标签；col 仅图标；fade 内部可滚动（矮窗口）"""
    its = ''
    for k, l, ico in NAVS:
        cur = ' aria-current="page"' if k == active else ''
        b = f'<span class="bd">{badge}</span>' if (k == 'queue' and badge) else ''
        lab = '' if mode == 'col' else f'<span>{l}</span>'
        its += f'<button class="nv"{cur} aria-label="{l}">{i(ico, 20, 1.6)}{lab}{b}</button>'
    cur = ' aria-current="page"' if active == 'settings' else ''
    lab = '' if mode == 'col' else '<span>设置</span>'
    st = f'<button class="nv"{cur} aria-label="设置">{i("set", 20, 1.6)}{lab}</button>'
    mon = '<div class="mon"><i></i>' + ('' if mode == 'col' else '监听') + '</div>'
    return f'<nav class="sd" aria-label="主导航"><div class="logo">{LOGO}</div><div class="nvs{" fade" if fade else ""}">{its}</div><div class="ft">{st}{mon}</div></nav>'

def win(content, w=1280, h=800, scale=0.66, theme='dark', active='queue', mode='full', fade=False, tag_=None, badge=3):
    nar = ' nar' if w < 1000 else ''
    col = ' col' if mode == 'col' else ''
    lab = tag_ or f'{"深色" if theme == "dark" else "浅色"} · {w}×{h}'
    cap = f'<div class="stage-h" style="margin-bottom:8px"><span>{lab}</span></div>'
    return f'''<div style="display:flex;flex-direction:column;flex:none">{cap}<div class="winwrap" data-t="{theme}" style="width:{round(w * scale)}px;height:{round(h * scale)}px"><div class="mw{nar}{col}" style="width:{w}px;height:{h}px;transform:scale({scale})">{side(active, mode, badge, fade)}<main class="mc">{content}</main></div></div></div>'''

def topbar(title, right=''):
    return f'<header class="tb"><h1>{title}</h1><span class="sp"></span>{right}</header>'

CATS = [('download', '下载', 'dl'), ('parse', '解析', 'link'), ('p2p', 'BT 与 ed2k', 'magnet'), ('network', '网络', 'globe'), ('accounts', '账号与 Cookie', 'user'), ('phone', '手机与浏览器扩展', 'phone'), ('auto', '自动化', 'bolt'), ('ai', 'AI', 'cpu'), ('writing', '写作', 'pen'), ('security', '安全与隐私', 'shield'), ('components', '组件', 'box'), ('diag', '诊断', 'pulse'), ('general', '通用', 'set')]

def settings(cur, title, cards, n=False, pre='', saved=True):
    cats = ''.join(f'<div class="cat" aria-current="{"true" if k == cur else "false"}">{i(ic_, 16)}<span class="ell">{nm}</span></div>' for k, nm, ic_ in CATS)
    srch = f'<div style="margin-bottom:8px">{inp("", "搜索设置", "search", st="", sfx=("" if n else kbd("Ctrl", "K")))}</div>'
    top = topbar('设置', chip('已自动保存', 'ok', True) if saved else '')
    return f'''{top}<div class="split"><aside class="cats" aria-label="设置分类">{srch}{cats}</aside><section class="stack" style="min-width:0"><div style="display:flex;align-items:center;gap:12px"><h2 style="font-size:var(--cc-fs-lg)">{title}</h2></div>{pre}{cards}</section></div><div class="fadeb"></div>'''

def pn(title, body, eb='', extra=''):
    e = f'<span class="eb">{eb}</span>' if eb else ''
    return f'<section class="pn"><div class="ph"><h2>{title}</h2>{e}<span class="sp"></span>{extra}</div>{body}</section>'
