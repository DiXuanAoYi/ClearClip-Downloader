// 量出每张 V2 画板的实际高度（写到 heights.json），并可选截图：node measure_v2.cjs [shots]
const { chromium } = require('playwright');
const fs = require('fs'), path = require('path');
const PRJ = path.resolve(__dirname, '../project');
(async () => {
  const keys = fs.readdirSync(PRJ).filter(f => f.startsWith('V2-') && f.endsWith('.dc.html'));
  const b = await chromium.launch();
  const out = {}; const shots = process.argv[2] === 'shots';
  const dir = path.resolve(__dirname, '../shots'); if (shots) fs.mkdirSync(dir, { recursive: true });
  for (const f of keys) {
    const p = await b.newPage({ viewport: { width: 1840, height: 1000 } });
    await p.route(/^https?:/, r => r.abort());
    await p.goto('file://' + path.join(PRJ, f));
    await p.addStyleTag({ content: 'x-dc{display:block}' });
    await p.waitForTimeout(150);
    const m = await p.evaluate(() => { const s = document.querySelector('.v2.sheet'); const r = s.getBoundingClientRect(); return { h: Math.ceil(r.height), w: Math.ceil(r.width), sw: document.documentElement.scrollWidth }; });
    out[f.replace('.dc.html', '')] = m.h;
    if (m.sw > m.w + 2) console.log('OVERFLOW-X', f, m.sw, m.w);
    if (shots) await p.screenshot({ path: path.join(dir, f.replace('.dc.html', '.png')), fullPage: true });
    await p.close();
  }
  await b.close();
  fs.writeFileSync(path.resolve(__dirname, 'heights.json'), JSON.stringify(out, null, 1));
  console.log(JSON.stringify(out));
})();
