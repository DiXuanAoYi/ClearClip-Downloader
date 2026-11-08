// 视频摘要面板的样式（浅色 + 深色覆盖）。由 vsum.js 使用。
;(() => {
  const CSS = `
    .panel { top: 0; right: 0; width: 400px; max-width: 100vw; height: 100vh; display: flex; flex-direction: column; background: #fbfaf8; color: #1d2420; border-left: 1px solid #cfd5d1; box-shadow: -8px 0 28px #0002; }
    .hd { display: flex; align-items: center; gap: 6px; padding: 8px 10px; border-bottom: 1px solid #e3e6e3; }
    .hd .t { flex: 1; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .hd button, .tools button { padding: 3px 8px; }
    .meta { padding: 0 10px 6px; font-size: 12px; color: #5d6862; display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
    .tabs { display: flex; border-bottom: 1px solid #e3e6e3; }
    .tabs button { flex: 1; background: transparent; color: #5d6862; border-radius: 0; border-bottom: 2px solid transparent; padding: 7px 0; font-weight: 400; }
    .tabs button.on { color: #b4561a; border-bottom-color: #b4561a; font-weight: 700; }
    .body { flex: 1; overflow: auto; padding: 10px; display: flex; flex-direction: column; }
    .body > :not(.selbar):not(#prog) { flex: 1 0 auto; display: flex; flex-direction: column; }
    .body > .sm { gap: 12px; }
    .body > #prog { flex: 0 0 auto; display: block }
    .body > #prog:empty { display: none }
    .tools { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-bottom: 8px; }
    select, input[type=text], textarea { border: 1px solid #cfd5d1; border-radius: 6px; padding: 4px 6px; background: #fff; color: inherit; min-width: 0; }
    textarea { width: 100%; resize: vertical; }
    .card { background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; white-space: pre-wrap; word-break: break-word; }
    .vcard .vh { display: flex; gap: 8px; align-items: baseline; flex-wrap: wrap; }
    .vcard .vf { margin-top: 4px; }
    .chips { display: flex; gap: 5px; flex-wrap: wrap; margin: 6px 0; }
    .chip { background: #d9762b1f; color: #8a3f0c; border-radius: 10px; padding: 1px 8px; font-size: 12px; }
    .ln { position: relative; display: grid; grid-template-columns: 48px 1fr; gap: 8px; padding: 5px 6px; border-radius: 8px; cursor: pointer; }
    .ln:hover { background: #d9762b14; }
    .ln.cur { background: #d9762b1f; }
    .ln.cur .tm { background: #b4561a; color: #fff; border-radius: 6px; padding: 0 6px; }
    .ln.hit { background: #f3d06a30; }
    .ln.hit.now { outline: 2px solid #d9762b; }
    mark { background: #ffe066; color: #1b1f1d; border-radius: 2px; padding: 0 1px; }
    .src { border: 1px solid #e3e6e3; background: #fbfaf8; border-radius: 8px; margin: 4px 0 0; padding: 4px 8px; font-size: 12px; color: #5d6862; cursor: pointer; }
    .warn { color: #a4561a; font-size: 12px; margin-top: 4px; }
    .ln .tm { color: #b4561a; font-variant-numeric: tabular-nums; font-weight: 700; font-size: 12px; line-height: 20px; align-self: start; justify-self: start; }
    .ln .tr { color: #5d6862; display: block; border-radius: 4px; transition: background .12s, color .12s; }
    .ln .mn { display: block; }
    .ln i.w { font-style: normal; border-radius: 3px; }
    .ln i.w:hover, .ln i.w.on { background: #f0a06455; box-shadow: 0 1px 0 #d9762b; }
    .ln mark.al { background: #f7c08a; color: #5a2a06; border-radius: 3px; padding: 0 2px; font-weight: 600; }
    .mute { color: #5d6862; font-size: 12px; }
    .cmbar { display: flex; height: 10px; border-radius: 5px; overflow: hidden; gap: 2px; margin: 6px 0; }
    .cmbar i { display: block; }
    .cmbar .p { background: #2f7d57; } .cmbar .u { background: #8a948f; } .cmbar .n { background: #b8472f; }
    .cmleg { display: flex; gap: 12px; font-size: 12px; color: #5d6862; flex-wrap: wrap; }
    .cmth { padding: 8px 0; border-top: 1px solid #e3e6e3; } .cmth:first-of-type { border-top: 0; padding-top: 2px; }
    .cmb { height: 6px; border-radius: 3px; background: #e3e6e3; margin: 4px 0; } .cmb i { display: block; height: 6px; border-radius: 3px; background: #d9762b; }
    .cq { background: #f4f5f4; border-radius: 6px; padding: 6px 8px; font-size: 12px; margin-top: 4px; white-space: pre-wrap; word-break: break-word; }
    .cq .m { display: flex; justify-content: space-between; align-items: center; color: #5d6862; margin-top: 2px; font-size: 11.5px; }
    .cq .m button { padding: 0 6px; font-size: 11.5px; }
    .tg { font-size: 11px; padding: 0 6px; border-radius: 4px; background: #d9762b1f; color: #8a3f0c; margin-right: 6px; white-space: nowrap; }
    .tg.ok { background: #2f7d5722; color: #1f6a45; } .tg.no { background: #b8472f22; color: #9a3320; }
    .cmq { display: flex; align-items: flex-start; gap: 4px; margin: 4px 0; }
    .warn { background: #fff4e5; border: 1px solid #f0c98e; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; }
    .bar { height: 4px; background: #e3e6e3; border-radius: 2px; overflow: hidden; margin: 6px 0; }
    .bar i { display: block; height: 100%; background: #d9762b; transition: width .3s; }
    .msg { margin-bottom: 8px; }
    .msg.u { text-align: right; }
    .msg .b { display: inline-block; text-align: left; max-width: 92%; padding: 6px 10px; border-radius: 10px; background: #fff; border: 1px solid #e3e6e3; white-space: pre-wrap; word-break: break-word; }
    .msg.u .b { background: #b4561a; color: #fff; border-color: #b4561a; }
    .ts { background: #fff; border: 1px solid #e3e6e3; color: #b4561a; padding: 0 6px; font-size: 12px; line-height: 20px; border-radius: 6px; font-weight: 700; font-variant-numeric: tabular-nums; }
    .ask { display: flex; gap: 6px; padding: 8px 10px; border-top: 1px solid #e3e6e3; }
    .ask input { flex: 1; }
    .svgbox { overflow: auto; border: 1px solid #e3e6e3; border-radius: 8px; background: #fff; max-height: 55vh; }
    .svgbox svg { display: block; width: 100%; height: auto; cursor: zoom-in; }
    .mapview { inset: 0; display: flex; flex-direction: column; background: #fbfaf8; color: #1d2420; }
    .mapview .bar2 { display: flex; gap: 6px; align-items: center; padding: 8px 10px; border-bottom: 1px solid #e3e6e3; flex-wrap: wrap; }
    .mapview .bar2 b { flex: 1; min-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .mapview .stage { flex: 1; overflow: hidden; position: relative; cursor: grab; touch-action: none; background: #fff; }
    .mapview .stage.drag { cursor: grabbing; }
    .mapview .stage > svg { position: absolute; left: 0; top: 0; transform-origin: 0 0; max-width: none; }
    .mapview .hint { font-size: 12px; color: #5d6862; padding: 4px 10px; }
    .ln .rec { visibility: hidden; position: absolute; right: 4px; top: 3px; background: #fff; border: 1px solid #e3e6e3; color: #b4561a; padding: 0 7px; font-size: 12px; }
    .ln:hover .rec { visibility: visible; }
    .selbar { position: sticky; top: 0; z-index: 2; background: #fff; border: 1px solid #d9762b; border-radius: 8px; padding: 6px 8px; margin-bottom: 8px; box-shadow: 0 2px 8px #0002; }
    .note { background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; padding: 8px 10px; margin-bottom: 8px; word-break: break-word; }
    .note .q { border-left: 3px solid #d9762b; padding-left: 8px; white-space: pre-wrap; }
    .note .x { display: flex; gap: 4px; flex-wrap: wrap; margin-top: 6px; align-items: center; }
    .note .x button { padding: 1px 7px; font-size: 12px; }
    .sm { display: flex; flex-direction: column; gap: 12px; }
    .sm .vid { display: grid; gap: 2px; }
    .sm .vid b { font-size: 14px; line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .sm .vmeta { display: flex; flex-wrap: wrap; gap: 2px 10px; color: #5d6862; font-size: 12px; }
    .sm .saved { color: #2f7a4d; }
    .sm .ctrl { display: grid; gap: 8px; }
    .sm .ctrl.stuck { position: sticky; top: -10px; z-index: 3; background: #fbfaf8; margin: -10px -10px 0; padding: 10px; border-bottom: 1px solid #e3e6e3; }
    .sm .seg { display: flex; gap: 2px; padding: 2px; background: #fff; border: 1px solid #e3e6e3; border-radius: 8px; overflow-x: auto; scrollbar-width: none; }
    .sm .seg button { flex: none; background: transparent; color: #5d6862; font-weight: 400; padding: 3px 10px; border-radius: 6px; white-space: nowrap; }
    .sm .seg button.on { background: #b4561a; color: #fff; font-weight: 700; }
    .sm .crow { display: flex; align-items: center; gap: 8px; }
    .sm .crow select { min-width: 0; max-width: 150px; }
    .sm .grow { flex: 1; }
    .sm .lead { background: #d9762b1a; border-radius: 10px; padding: 10px 12px; }
    .sm .lead .k { font-size: 11px; letter-spacing: .1em; color: #b4561a; font-weight: 700; margin-bottom: 2px; }
    .sm .lead p { margin: 0; font-size: 1.15em; line-height: 1.55; font-weight: 600; }
    .sm .sh { display: flex; align-items: baseline; gap: 4px; margin: 0 0 6px; font-size: 12px; font-weight: 700; letter-spacing: .08em; color: #5d6862; }
    .sm .sh em { font-style: normal; font-weight: 400; letter-spacing: 0; }
    .sm .sh .lnk { margin-left: auto; background: transparent; color: #b4561a; padding: 0; font-weight: 400; }
    .sm .prose p { margin: 0 0 .65em; line-height: 1.8; }
    .sm .prose p:last-child { margin: 0; }
    .sm .kw { display: flex; flex-wrap: wrap; gap: 6px; }
    .sm .kw span { border: 1px solid #e3e6e3; background: #fff; border-radius: 6px; padding: 0 8px; font-size: 12px; line-height: 22px; }
    .sm .kw span::before { content: "#"; color: #b4561a; margin-right: 2px; }
    .sm .pts { list-style: none; margin: 0; padding: 0; position: relative; display: grid; gap: 2px; }
    .sm .pts::before { content: ""; position: absolute; left: 25px; top: 10px; bottom: 10px; border-left: 1px dashed #cfd5d1; }
    .sm .pts li { position: relative; display: grid; grid-template-columns: 52px 1fr; gap: 8px; padding: 6px 4px; border-radius: 8px; }
    .sm .pts li.go { cursor: pointer; }
    .sm .pts li.go:hover { background: #d9762b14; }
    .sm .pts .tm { align-self: start; justify-self: start; position: relative; background: #fff; border: 1px solid #e3e6e3; color: #b4561a; font-variant-numeric: tabular-nums; font-weight: 700; font-size: 12px; line-height: 20px; padding: 0 6px; border-radius: 6px; }
    .sm .pts b { display: block; line-height: 1.5; }
    .sm .pts .d { color: #5d6862; }
    .acts { position: sticky; bottom: -10px; z-index: 2; margin-top: auto; flex: none; display: flex; gap: 6px; padding: 8px 0 2px; background: #fbfaf8; }
    .acts button:first-child { flex: 1; }
    .stick { position: sticky; top: -10px; z-index: 2; display: grid; gap: 8px; background: #fbfaf8; margin: -10px -10px 8px; padding: 10px; border-bottom: 1px solid #e3e6e3; }
    .stick .tools { margin: 0; }
    .chs { display: grid; gap: 2px; position: relative; }
    .chs::before { content: ""; position: absolute; left: 27px; top: 12px; bottom: 12px; border-left: 1px dashed #cfd5d1; }
    .chs .ln { grid-template-columns: 52px 1fr; }
    .chs .ln .tm { background: #fff; border: 1px solid #e3e6e3; border-radius: 6px; padding: 0 6px; position: relative; }
    .chs .ln.cur .tm { background: #b4561a; color: #fff; border-color: #b4561a; }
    .chs .gist { display: block; color: #5d6862; }
    .quote { border-left: 3px solid #d9762b; padding: 2px 0 2px 10px; margin: 4px 0 0; color: #5d6862; font-size: 12px; }
    .sugg { display: grid; gap: 6px; }
    .sugg button { text-align: left; display: flex; justify-content: space-between; padding: 8px 12px; border-radius: 10px; font-weight: 400; }
    .sugg button::after { content: "›"; color: #b4561a; }
    .svgbox { position: relative; }
    .svgbox .zoomhint { position: absolute; left: 10px; bottom: 6px; font-size: 11px; color: #5d6862; pointer-events: none; }
    details.acc { border: 1px solid #e3e6e3; border-radius: 10px; background: #fff; }
    details.acc summary { padding: 8px 10px; cursor: pointer; font-weight: 700; }
    details.acc pre { margin: 0; padding: 0 10px 10px; font: 12px/1.7 ui-monospace, Menlo, Consolas, monospace; color: #5d6862; white-space: pre-wrap; }
    .grp { background: #fff; border: 1px solid #e3e6e3; border-radius: 10px; overflow: hidden; margin-bottom: 10px; }
    .grp .gh { padding: 6px 12px; font-size: 12px; font-weight: 700; letter-spacing: .08em; color: #5d6862; background: #fbfaf8; border-bottom: 1px solid #e3e6e3; }
    .grp .gb { padding: 8px 12px; display: flex; flex-direction: column; gap: 8px; }
    .grp .gb .card { background: #fbfaf8; margin: 0; }
    .note .x { opacity: .95; }
    .sm .empty { display: grid; gap: 6px; justify-items: center; text-align: center; padding: 16px 8px; }
    .sm .empty p { margin: 0; max-width: 30ch; }
    .sm .warn { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 0; }
    .sm .warn span { flex: 1; min-width: 160px; }
    h4 { margin: 10px 0 4px; font-size: 13px; }
    ul { margin: 4px 0; padding-left: 18px; }
  `

  const DARK = `
      .panel, .mapview { background: #1d2420; color: #eef1ee; border-left-color: #3a443e; }
      .mapview .stage { background: #1d2420; }
      .mapview .bar2 { border-color: #3a443e; }
      .hd, .meta, .tabs, .ask { border-color: #3a443e; }
      .selbar, .note, .grp, details.acc, .ts, .ln .rec, .chs .ln .tm { background: #262f2a; border-color: #3a443e; }
      .stick, .grp .gh, .src { background: #1d2420; border-color: #3a443e; }
      .grp .gb .card { background: #1d2420; }
      .ln .tm, .ts, .ln.cur .tm, .sugg button::after { color: #f0a064; }
      .ln.cur .tm { background: #f0a064; color: #1d2420; }
      .ln.cur { background: #e28a4524; }
      .quote, .chs .gist, details.acc pre, .grp .gh { color: #a9b3ad; }
      .chs::before { border-color: #3a443e; }
      .chs .ln.cur .tm { background: #f0a064; color: #1d2420; border-color: #f0a064; }
      .card, .msg .b, select, input[type=text], textarea { background: #262f2a; border-color: #3a443e; color: #eef1ee; }
      button.sec { background: #262f2a; color: #eef1ee; border-color: #3a443e; }
      .ln .tr, .meta, .mute, .src { color: #a9b3ad; }
      .ln i.w:hover, .ln i.w.on { background: #f0a06440; } .ln mark.al { background: #8a4a1c; color: #ffe2c4; }
      .warn { background: #3a2f1e; border-color: #6b5530; }
      .cq { background: #1d2420; } .chip { color: #f0a064; background: #e28a4529; } .cmb, .cmth { background: #3a443e; border-color: #3a443e; } .cmth { background: none; } .cmleg, .cq .m { color: #a9b3ad; }
      .tg { color: #f0a064; } .tg.ok { color: #6cc08a; } .tg.no { color: #ee917b; }
      .sm .seg, .sm .kw span, .sm .pts .tm { background: #262f2a; border-color: #3a443e; }
      .sm .seg button, .sm .vmeta, .sm .sh, .sm .pts .d { color: #a9b3ad; }
      .sm .seg button.on { background: #f0a064; color: #1d2420; }
      .sm .lead .k, .sm .pts .tm, .sm .sh .lnk { color: #f0a064; }
      .sm .saved { color: #6cc08a; }
      .acts, .sm .ctrl.stuck { background: #1d2420; border-color: #3a443e; }
      .sm .pts::before { border-color: #3a443e; }
  `
  globalThis.ccVsumStyle = { CSS, DARK }
})()
