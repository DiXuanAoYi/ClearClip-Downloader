// 听写：把一句句定稿的话接成一段。中日韩直接连，英文等加空格（和后端 dictate::join_finals 同一个规则）。
const isCjk = (c: string) => /[⺀-鿿豈-﫿＀-￯가-힯]/.test(c)

export function joinFinals(parts: string[]): string {
  let out = ''
  for (const raw of parts) {
    const p = raw.trim()
    if (!p) continue
    if (!out) out = p
    else if (isCjk(out[out.length - 1]) || isCjk(p[0])) out += p
    else out += ' ' + p
  }
  return out
}
