import json, re, os
from lib import *
import pages_a as A, pages_b as B, pages_c as C, pages_d as D
P = '../project/'
boards = []   # (file, title, h, html)
def add(f, t, h, html): boards.append((f, t, h, html))

# 解析（保留原文件，只换导航）
main = open(P+'Main.dc.html').read()
main = re.sub(r'<nav class="rail".*?</nav>', nav('parse'), main, flags=re.S)
open(P+'Main.dc.html','w').write(main)
boards.append(('Main.dc.html','解析',1300,None))

add('ParseList.dc.html','解析 · 列表与批量',1240,A.p_parselist())
add('Queue.dc.html','下载队列',980,A.p_queue())
add('Subs.dc.html','订阅',940,A.p_subs())
add('Live.dc.html','直播',860,A.p_live())
add('Screen.dc.html','屏幕翻译',1680,B.p_screen())
add('Library.dc.html','媒体库',1560,B.p_library())
add('FileTrans.dc.html','文件翻译',1020,B.p_filetr())
add('Vocab.dc.html','生词与摘录',980,B.p_vocab())
add('Toolbox.dc.html','工具箱',1230,B.p_toolbox())
add('Editor.dc.html','工具箱 · 剪辑',1160,B.p_editor())
add('Overlays.dc.html','浮层与小窗',1100,B.p_overlays())
add('Write.dc.html','清影写作',2640,D.p_write())
add('ExtPopup.dc.html','扩展 · 弹窗',1400,D.p_ext_popup())
add('ExtSettings.dc.html','扩展 · 设置（横向导航）',2060,D.p_ext_settings())
add('ExtPage.dc.html','扩展 · 页面内浮层',1560,D.p_ext_page())
add('Phone.dc.html','手机互传',1500,D.p_phone())
add('Small.dc.html','小窗口与悬浮条',900,D.p_small())
sets = [('Settings.dc.html','设置 · 下载',C.s_download,1500),('Settings-parse.dc.html','设置 · 解析',C.s_parse,1470),('Settings-p2p.dc.html','设置 · BT 与 ed2k',C.s_p2p,900),('Settings-network.dc.html','设置 · 网络',C.s_network,900),('Settings-accounts.dc.html','设置 · 账号与 Cookie',C.s_accounts,900),('Settings-phone.dc.html','设置 · 手机与扩展',C.s_phone,900),('Settings-auto.dc.html','设置 · 自动化',C.s_auto,900),('Settings-ai.dc.html','设置 · AI',C.s_ai,1100),('Settings-writing.dc.html','设置 · 写作',C.s_writing,900),('Settings-security.dc.html','设置 · 安全与隐私',C.s_security,900),('Settings-components.dc.html','设置 · 组件',C.s_components,1000),('Settings-diag.dc.html','设置 · 诊断',C.s_diag,900),('Settings-general.dc.html','设置 · 通用',C.s_general,1100)]
for f,t,fn,h in sets:
    html = fn()
    html = html.replace(f'"height":1000}}', f'"height":{h}}}').replace('.shell{min-height:1000px}', f'.shell{{min-height:{h}px}}')
    add(f,t,h,html)
for f,t,h,html in boards:
    if html: open(P+f,'w').write(html)

# canvas.json（3 列网格，按行高叠放）
cols=3; X=1440; gap=120
rows=[boards[i:i+cols] for i in range(0,len(boards),cols)]
y=0; out={}; order=[]
for r in rows:
    for ci,(f,t,h,_) in enumerate(r):
        out[f]={"x":ci*X,"y":y,"w":1360,"h":h,"title":t,"expand":"fill","is_interactive":True}; order.append(f)
    y += max(b[2] for b in r)+gap
cj=json.load(open(P+'canvas.json'))
cj['boards']=out; cj['order']=order
cj['notes']={"t1":{"x":0,"y":-280,"text":"清影 ClearClip · 全部页面设计（冷钢深空 + 镜头琥珀）","kind":"title1","maxW":4200}}
json.dump(cj,open(P+'canvas.json','w'),ensure_ascii=False,indent=2)
print(len(boards),'boards', y)
print(json.dumps({f:f for f,*_ in boards}))
