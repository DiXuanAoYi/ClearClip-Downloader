//! 网页控制台：在手机 / 其他电脑的浏览器里查看和控制下载（局域网内，需要访问令牌）。
//! 页面只是 `/api/v1/*` 的一个客户端。

pub fn page(token: &str) -> String {
    // 令牌只会是字母数字，仍然做一次转义，避免被拿来注入脚本
    let safe: String = token.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    PAGE.replace("__TOKEN__", &safe)
}

const PAGE: &str = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>清影 控制台</title>
<style>
:root{--bg:#f4f5f4;--card:#fff;--fg:#1d2420;--mute:#69746f;--acc:#d9762b;--line:#d9dedb;--ok:#2f8a57;--err:#c2453b}
@media (prefers-color-scheme:dark){:root{--bg:#14181a;--card:#22292c;--fg:#e7ecea;--mute:#8c9894;--line:#2f383c;--acc:#f08a3c;--ok:#5cc28a;--err:#ec7a70}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"PingFang SC","Microsoft YaHei",sans-serif}
main{max-width:860px;margin:0 auto;padding:16px 16px 48px}
h1{font-size:18px;margin:4px 0}h1 b{color:var(--acc)}h2{font-size:13px;color:var(--mute);margin:20px 0 8px;font-weight:500}
.bar{display:flex;gap:16px;flex-wrap:wrap;color:var(--mute);font-size:13px;margin-bottom:12px}.bar b{color:var(--fg)}
.row{display:flex;gap:8px}input[type=text]{flex:1;min-width:0;padding:10px 12px;border:1px solid var(--line);border-radius:8px;background:var(--card);color:var(--fg);font:inherit}
button{padding:8px 14px;border:1px solid var(--line);border-radius:8px;background:var(--card);color:var(--fg);font:inherit;cursor:pointer}
button.p{background:var(--acc);border-color:var(--acc);color:#fff}button.s{padding:3px 9px;font-size:12px}button:disabled{opacity:.55}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 12px;margin-bottom:8px}
.t{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.m{color:var(--mute);font-size:12px;display:flex;gap:10px;flex-wrap:wrap;align-items:center;margin-top:2px}
.prog{height:5px;background:var(--line);border-radius:3px;overflow:hidden;margin:6px 0}.prog i{display:block;height:100%;background:var(--acc)}
.ok{color:var(--ok)}.err{color:var(--err)}#msg{min-height:20px;margin:6px 0;font-size:13px;color:var(--mute)}
.ops{margin-left:auto;display:flex;gap:6px}
</style></head><body><main>
<h1>清影 <b>控制台</b></h1>
<div class="bar" id="bar">连接中…</div>
<div class="row"><input type="text" id="url" placeholder="粘贴链接，回车添加"><button class="p" id="add">添加</button></div>
<div id="msg"></div>
<div class="row" style="margin-bottom:8px"><button class="s" id="pa">全部暂停</button><button class="s" id="ra">全部继续</button><button class="s" id="cf">清除已完成</button></div>
<h2>下载任务</h2><div id="tasks"></div>
<h2>媒体库</h2>
<div class="row"><input type="text" id="q" placeholder="搜索标题、作者或标签"><button id="go">搜索</button></div>
<div id="lib" style="margin-top:8px"></div>
</main><script>
const TOKEN="__TOKEN__";const $=id=>document.getElementById(id);
async function api(path,method="GET",body){const r=await fetch(path,{method,headers:{"X-Token":TOKEN,"Content-Type":"application/json"},body:body?JSON.stringify(body):undefined});const j=await r.json().catch(()=>({}));if(!r.ok)throw new Error(j.error||("HTTP "+r.status));return j}
const bytes=n=>{if(n==null)return"—";const u=["B","KB","MB","GB"];let i=0;while(n>=1024&&i<3){n/=1024;i++}return n.toFixed(i?1:0)+" "+u[i]};
const ST={running:"下载中",queued:"等待中",paused:"已暂停",failed:"失败",done:"已完成",canceled:"已取消"};
function esc(s){const d=document.createElement("div");d.textContent=s??"";return d.innerHTML}
async function act(id,a){try{await api("/api/v1/tasks/"+id+"/"+a,"POST");load()}catch(e){$("msg").textContent=e.message}}
window.act=act;
async function load(){try{
const s=await api("/api/v1/status");const t=s.tasks;
$("bar").innerHTML="<span>版本 <b>"+esc(s.version)+"</b></span><span>下载中 <b>"+t.running+"</b></span><span>等待 <b>"+t.queued+"</b></span><span>失败 <b>"+t.failed+"</b></span><span>速度 <b>"+bytes(t.speed)+"/s</b></span>"+(s.recording?"<span class=ok>正在录制直播</span>":"");
const r=await api("/api/v1/tasks");const el=$("tasks");
el.innerHTML=r.tasks.length?"":"<div class=m>没有任务</div>";
for(const k of r.tasks.slice().reverse().slice(0,80)){const pct=k.total?Math.min(100,Math.round(k.received*100/k.total)):0;
const btn=k.status==="running"||k.status==="queued"?`<button class=s onclick="act(${k.id},'pause')">暂停</button><button class=s onclick="act(${k.id},'cancel')">取消</button>`:k.status==="paused"||k.status==="failed"?`<button class=s onclick="act(${k.id},'resume')">${k.status==="failed"?"重试":"继续"}</button><button class=s onclick="act(${k.id},'remove')">移除</button>`:`<button class=s onclick="act(${k.id},'remove')">移除</button>`;
el.insertAdjacentHTML("beforeend",`<div class=card><div class=t>${esc(k.fileName||k.title)}</div>${k.status==="running"?`<div class=prog><i style="width:${pct}%"></i></div>`:""}<div class=m><span class="${k.status==="done"?"ok":k.status==="failed"?"err":""}">${ST[k.status]||k.status}${k.status==="running"?" "+pct+"%":""}</span><span>${esc(k.assetLabel)}</span><span>${bytes(k.received)}${k.total?" / "+bytes(k.total):""}</span>${k.speed?`<span>${bytes(k.speed)}/s</span>`:""}${k.error?`<span class=err>${esc(k.error)}</span>`:""}<span class=ops>${btn}</span></div></div>`)}
}catch(e){$("bar").textContent="连接失败："+e.message}}
async function add(){const v=$("url").value.trim();if(!v)return;try{await api("/api/v1/add","POST",{text:v});$("url").value="";$("msg").textContent="已发送，清影开始处理";load()}catch(e){$("msg").textContent=e.message}}
$("add").onclick=add;$("url").onkeydown=e=>{if(e.key==="Enter")add()};
$("pa").onclick=()=>api("/api/v1/tasks/pause_all","POST").then(load);$("ra").onclick=()=>api("/api/v1/tasks/resume_all","POST").then(load);$("cf").onclick=()=>api("/api/v1/tasks/clear_finished","POST").then(load);
async function search(){try{const r=await api("/api/v1/library?limit=40&q="+encodeURIComponent($("q").value));$("lib").innerHTML=r.items.length?"":"<div class=m>没有结果</div>";
for(const i of r.items)$("lib").insertAdjacentHTML("beforeend",`<div class=card><div class=t>${i.favorite?"★ ":""}${esc(i.title)}</div><div class=m><span>${esc(i.platform)}</span><span>${esc(i.author)}</span><span>${bytes(i.size)}</span>${i.tags.map(t=>"<span>#"+esc(t)+"</span>").join("")}${i.exists?"":"<span class=err>文件已丢失</span>"}</div></div>`)}catch(e){$("msg").textContent=e.message}}
$("go").onclick=search;$("q").onkeydown=e=>{if(e.key==="Enter")search()};
load();search();setInterval(load,2000);
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_embedded_safely() {
        let p = page("abc123");
        assert!(p.contains("const TOKEN=\"abc123\";"));
        let evil = page("x\";alert(1);//");
        assert!(evil.contains("const TOKEN=\"xalert1\";"), "quotes and punctuation are stripped");
        assert!(p.contains("/api/v1/tasks"));
    }
}
