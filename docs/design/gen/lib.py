import itertools
_id = itertools.count(1)
def uid(p='x'): return f'{p}{next(_id)}'

def ic(d, s=18, sw=1.7):
    return f'<svg width="{s}" height="{s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="{sw}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="{d}"/></svg>'

I = {
 'link': 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
 'dl': 'M12 4v11M7 11l5 5 5-5M5 20h14',
 'bell': 'M6 16v-5a6 6 0 1 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0',
 'live': 'M10 12a2 2 0 1 0 4 0a2 2 0 1 0-4 0M7.5 7.5a6 6 0 0 0 0 9M16.5 7.5a6 6 0 0 1 0 9',
 'screen': 'M4 5h16v11H4zM8 20h8M9 9h6M9 12h4',
 'lib': 'M4 5h16v14H4zM4 15l4-4 4 4 3-3 5 5',
 'file': 'M7 3h7l5 5v13H7zM14 3v5h5M10 13h6M10 17h4',
 'vocab': 'M7 4h10v17l-5-4-5 4z',
 'tools': 'M4 8h16v11H4zM9 8V5h6v3M4 13h16',
 'set': 'M5 7h14M5 12h14M5 17h14M9 5v4M15 10v4M8 15v4',
 'search': 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM20 20l-4-4',
 'plus': 'M12 5v14M5 12h14',
 'play': 'M8 5.5v13l11-6.5-11-6.5Z',
 'pause': 'M8 5v14M16 5v14',
 'x': 'M6 6l12 12M18 6 6 18',
 'check': 'M5 12l5 5 9-10',
 'trash': 'M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13',
 'folder': 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z',
 'refresh': 'M20 11a8 8 0 0 0-14-4L4 9M4 4v5h5M4 13a8 8 0 0 0 14 4l2-2M20 20v-5h-5',
 'cut': 'M6 7a2.5 2.5 0 1 0 0 .01M6 17a2.5 2.5 0 1 0 0 .01M8 8.500 20 18M8 15.500 20 6',
 'lock': 'M6 11h12v9H6zM9 11V8a3 3 0 0 1 6 0v3',
 'phone': 'M8 3h8a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1ZM11 18h2',
 'globe': 'M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18M12 3a9 9 0 1 0 0 18',
 'user': 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM4 20a8 8 0 0 1 16 0',
 'cpu': 'M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4',
 'shield': 'M12 3 5 6v5c0 4.500 3 8 7 10 4-2 7-5.500 7-10V6l-7-3Z',
 'pulse': 'M3 12h4l2-6 4 12 2-6h6',
 'bolt': 'M13 3 5 14h6l-1 7 8-11h-6z',
 'pen': 'M4 20l4-1 11-11-3-3L5 16zM14 6l3 3',
 'box': 'M12 3 4 7.500v9L12 21l8-4.500v-9L12 3ZM4 7.500 12 12l8-4.500M12 12v9',
 'magnet': 'M6 4v9a6 6 0 0 0 12 0V4h-4v9a2 2 0 0 1-4 0V4zM6 8h4M14 8h4',
 'tr': 'M4 5h9M8.500 3v2M6 5c.6 3 2.600 5.400 5.500 6.800M12 5c-.6 3-3 6-7 8M13 20l4-9 4 9M14.500 17h5',
 'star': 'M12 3l2.800 5.700 6.200.9-4.500 4.400 1.100 6.200L12 17.200 6.400 20.200l1.100-6.200L3 9.600l6.200-.9z',
 'grid': 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
 'list': 'M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01',
 'more': 'M5 12h.01M12 12h.01M19 12h.01',
 'edit': 'M4 20h4L19 9l-4-4L4 16zM13 7l4 4',
 'mic': 'M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3ZM5 11a7 7 0 0 0 14 0M12 18v3',
 'img': 'M4 5h16v14H4zM4 15l4-4 4 4 3-3 5 5',
 'cmd': 'M9 9h6v6H9zM9 9V6a3 3 0 1 0-3 3M15 9V6a3 3 0 1 1 3 3M9 15v3a3 3 0 1 1-3-3M15 15v3a3 3 0 1 0 3-3',
 'book': 'M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3zM5 17a3 3 0 0 1 3-3h11',
}
NAVS = [('parse','解析','link'),('queue','下载队列','dl'),('subs','订阅','bell'),('live','直播','live'),('screen','屏幕翻译','screen'),('library','媒体库','lib'),('filetr','文件翻译','file'),('vocab','生词与摘录','vocab'),('tools','工具箱','tools')]

def nav(active):
    out = ['<nav class="rail" aria-label="主导航">',
     '<div class="logo" role="img" aria-label="清影"><svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 3l3.5 6.1M20.2 8.2l-6.8 1M20.2 15.8 15 11M12 21l-3.5-6.1M3.8 15.8l6.8-1M3.8 8.2 9 13"/></svg></div>']
    def item(k,l,i):
        cur = ' aria-current="page"' if k==active else ''
        return f'<button class="nav-i"{cur}>{ic(I[i],22,1.6)}<span>{l}</span></button>'
    for k,l,i in NAVS: out.append(item(k,l,i))
    out.append('<div class="nav-gap"></div>')
    out.append(item('settings','设置','set'))
    out.append('</nav>')
    return '\n'.join(out)

FONTS = '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Chakra+Petch:wght@500;600;700&family=JetBrains+Mono:wght@400;500&family=Noto+Sans+SC:wght@400;500;700&display=swap">'

def page(title, active, h, topbar, body, css='', min_h=None):
    mh = f'.shell{{min-height:{h}px}}' if h else ''
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
{mh}
{css}
</style>
</helmet>
<div class="cc" data-theme="{{{{theme}}}}">
<div class="shell">
{nav(active)}
<main class="main">
<header class="topbar">
<h1>{title}</h1>
{topbar}
</header>
{body}
</main>
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

# ---------- 小部件 ----------
def chip(t, c=''): return f'<span class="chip {c}">{t}</span>'
def dot(c=''): return f'<i class="dot {c}"></i>'
def btn(t, c='', icon=None, a=''):
    i = ic(I[icon]) if icon else ''
    return f'<button class="btn {c}" {a}>{i}{t}</button>'
def sw(on=True, label='开关'):
    return f'<label class="sw"><input type="checkbox"{" checked" if on else ""} aria-label="{label}"><span></span></label>'
def seg(opts, sel=0, label='选择'):
    n = uid('g')
    s = ''.join(f'<label><input type="radio" name="{n}"{" checked" if i==sel else ""}><span>{o}</span></label>' for i,o in enumerate(opts))
    return f'<span class="seg" role="radiogroup" aria-label="{label}">{s}</span>'
def sel(opts, cur=0, label='选择', w=None):
    o = ''.join(f'<option{" selected" if i==cur else ""}>{x}</option>' for i,x in enumerate(opts))
    st = f' style="width:{w}px"' if w else ''
    return f'<select aria-label="{label}"{st}>{o}</select>'
def field(v='', ph='', icon=None, label='输入', w=None, right=''):
    i = ic(I[icon]) if icon else ''
    st = f' style="width:{w}px;flex:none"' if w else ''
    return f'<label class="field"{st}>{i}<input type="text" value="{v}" placeholder="{ph}" aria-label="{label}">{right}</label>'
def stepper(v, label='数值'):
    return f'<span class="step"><button aria-label="减少">−</button><b>{v}</b><button aria-label="增加">+</button></span>'
def row(t, small='', ctl=''):
    sm = f'<small>{small}</small>' if small else ''
    c = f'<div class="ctl">{ctl}</div>' if ctl else ''
    return f'<div class="row"><div class="t"><b>{t}</b>{sm}</div>{c}</div>'
def card(title, eb='', body='', extra='', cls=''):
    e = f'<span class="eyebrow">{eb}</span>' if eb else ''
    return f'<article class="panel {cls}"><div class="panel-h"><h2>{title}</h2>{e}<span class="spacer"></span>{extra}</div>{body}</article>'
def tabs(names, cur=0, badges=None):
    badges = badges or {}
    return '<div class="tabs" role="tablist">' + ''.join(f'<button class="tab" role="tab" aria-selected="{"true" if i==cur else "false"}">{n}{(f"<span class=n>{badges[i]}</span>" if i in badges else "")}</button>' for i,n in enumerate(names)) + '</div>'
def stat(n, l, on=False): return f'<div class="stat{" on" if on else ""}"><b>{n}</b><span>{l}</span></div>'
def prog(p, c=''): return f'<div class="prog {c}"><i style="width:{p}%"></i></div>'
def thumb(t='', dur=''):
    d = f'<span class="dur mono">{dur}</span>' if dur else ''
    return f'<div class="thumb {t}">{d}</div>'
def check(t, on=True): return f'<label class="check"><input type="checkbox"{" checked" if on else ""}><span>{t}</span></label>'
def tr(cols, cells, th=False):
    c = ' '.join(cols)
    return f'<div class="tr{" th" if th else ""}" style="--cols:{c}">' + ''.join(f'<span>{x}</span>' for x in cells) + '</div>'

CATS = [('download','下载','dl'),('parse','解析','link'),('p2p','BT 与 ed2k','magnet'),('network','网络','globe'),('accounts','账号与 Cookie','user'),('phone','手机与浏览器扩展','phone'),('auto','自动化','bolt'),('ai','AI','cpu'),('writing','写作','pen'),('security','安全与隐私','shield'),('components','组件','box'),('diag','诊断','pulse'),('general','通用','set')]
def settings_page(file, cur, title, cards, intro="", nct=2, hh=1000):
    cats = ''.join(f'<button class="cat" aria-current="{"true" if k==cur else "false"}">{ic(I[i])}<span>{n}</span></button>' for k,n,i in CATS)
    body = f'''<div class="split">
<aside class="cats sticky" aria-label="设置分类">
<label class="field" style="margin-bottom:10px">{ic(I['search'])}<input type="text" placeholder="搜索设置" aria-label="搜索设置"><span class="kbd">Ctrl K</span></label>
{cats}
</aside>
<section aria-label="{title}设置">
<div class="toolbar" style="margin-bottom:14px"><h2 style="font-size:18px">{title}</h2><span class="spacer"></span><span class="eyebrow">设置页排布</span>{seg(['自动平衡','按顺序','单列'],0,'设置页排布')}</div>
{('<p class="note" style="margin-bottom:14px">'+intro+'</p>') if intro else ''}
<div class="masonry" style="column-count:{nct}">
{''.join(cards)}
</div>
<div class="toolbar"><span class="mono" style="color:var(--faint);font-size:12px">修改即时生效，无需手动保存</span><span class="spacer"></span>{btn('恢复默认','sm')}</div>
</section>
</div>'''
    top = f'<span class="sub mono">{title}</span><span class="spacer"></span>{chip(dot()+"已自动保存","cyan")}'
    top = f'<span class="spacer"></span>{chip(dot()+"已自动保存","cyan")}'
    return page("设置","settings",hh,top,body)
