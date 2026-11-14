# 设计系统 v2 画板生成：只新增 V2-*.dc.html、v2.css 引用，并把它们追加进 canvas.json；不动旧画板。
# 用法：cd gen && python3 build_v2.py [heights.json]   （heights.json 由 measure_v2.cjs 产生，第一遍不带）
import json, os, sys
from v2lib import *
import v2_spec as S
import v2_pages as P

PRJ = '../project/'
W = 1840
FONTS = '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Chakra+Petch:wght@500;600;700&family=JetBrains+Mono:wght@400;500&family=Noto+Sans+SC:wght@400;500;700&display=swap">'

def wrap(title, html, h):
    html = html.replace('class="v2 sheet"', f'class="v2 sheet" style="width:{W}px;box-sizing:border-box"', 1)
    return f'''<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>{title}</title>
<script src="./support.js"></script>
<link rel="stylesheet" href="v2.css">
</head>
<body>
<x-dc>
<helmet>
{FONTS}
<style>
body {{ margin: 0; }}
</style>
</helmet>
{html}
</x-dc>
<script type="text/x-dc" data-dc-script data-props='{{"$preview":{{"width":{W},"height":{h}}}}}'>
class Component extends DCLogic {{
  renderVals() {{ return {{}}; }}
}}
</script>
</body>
</html>
'''

BOARDS = [
    ('V2-Overview', '总览与待拍板事项', S.board_overview),
    ('V2-D', 'D 实施清单与验收', S.board_d),
    ('V2-DS-1', '设计系统 · 度量与动效', S.board_ds1),
    ('V2-DS-2', '设计系统 · 色彩与令牌清单', S.board_ds2),
    ('V2-CMP-1', '组件 · 按钮 分段 链接 选择', S.board_cmp1),
    ('V2-CMP-2', '组件 · 字段 空状态 骨架 对话框 提示', S.board_cmp2),
] + [(k, t, fn) for k, t, fn in P.PAGES] + [
    ('V2-C11', 'C11 加载 · 错误 · 反馈', S.board_c11),
    ('V2-C12', 'C12 硬编码色对照', S.board_c12),
]
# 画板在画布上的排列：两列，按下面的顺序（总览/清单、设计系统、组件、C1…C12）
ORDER = ['V2-Overview', 'V2-D', 'V2-DS-1', 'V2-DS-2', 'V2-CMP-1', 'V2-CMP-2', 'V2-C1', 'V2-C2', 'V2-C3', 'V2-C4', 'V2-C5', 'V2-C6', 'V2-C7', 'V2-C8', 'V2-C9', 'V2-C10', 'V2-C11', 'V2-C12']

if __name__ == '__main__':
    hs = json.load(open(sys.argv[1])) if len(sys.argv) > 1 else {}
    built = {}
    for key, title, fn in BOARDS:
        html = fn()
        h = hs.get(key, 1400)
        open(PRJ + key + '.dc.html', 'w').write(wrap('清影 · ' + title, html, h))
        built[key] = (title, h)
    cj = json.load(open(PRJ + 'canvas.json'))
    for k in [k for k in cj['boards'] if k.startswith('V2-')]:
        del cj['boards'][k]
    cj['order'] = [o for o in cj['order'] if not o.startswith('V2-')]
    y0 = max(b['y'] + b['h'] for b in cj['boards'].values()) + 480
    y = y0
    gap = 120
    for r in range(0, len(ORDER), 2):
        row = ORDER[r:r + 2]
        for c, key in enumerate(row):
            title, h = built[key]
            cj['boards'][key + '.dc.html'] = {'x': c * (W + gap), 'y': y, 'w': W, 'h': h, 'title': title, 'is_interactive': False}
            cj['order'].append(key + '.dc.html')
        y += max(built[k][1] for k in row) + gap
    cj['notes']['t2'] = {'x': 0, 'y': y0 - 300, 'text': '设计系统 v2 · UI 提升到 8–9 分（待产品负责人确认，确认前不改代码）', 'kind': 'title1', 'maxW': 2 * W + gap}
    json.dump(cj, open(PRJ + 'canvas.json', 'w'), ensure_ascii=False, indent=2)
    print('built', len(built), 'boards; y0 =', y0, 'end y =', y)
