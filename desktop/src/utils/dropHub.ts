// 从资源管理器拖文件进窗口：整个窗口只有一个原生的拖放事件（App.vue 里接），
// 哪个页面要自己接收（比如剪辑页把文件拖到时间线的某一行），就在这里登记；没人接收的才当成“导入链接文件”。
// 坐标都是 CSS 像素（相对窗口左上角）。这个文件不要引入别的运行时模块。

export interface DropClaimant {
  /** 这个位置归我吗（页面没显示、鼠标不在我的范围里就返回 false） */
  accepts(x: number, y: number): boolean
  /** 文件在我的范围里移动（用来高亮落点） */
  over(x: number, y: number): void
  /** 文件离开了我的范围 */
  leave(): void
  /** 松手 */
  drop(paths: string[], x: number, y: number): void
}

const claimants = new Set<DropClaimant>()
let current: DropClaimant | null = null

/** 登记一个接收者，返回取消登记的函数。 */
export function registerDrop(c: DropClaimant): () => void {
  claimants.add(c)
  return () => {
    claimants.delete(c)
    if (current === c) current = null
  }
}

function pick(x: number, y: number): DropClaimant | null {
  for (const c of claimants) if (c.accepts(x, y)) return c
  return null
}

/** 文件拖过窗口。返回 true 表示有页面接收（这时不要显示“松开以导入链接”）。 */
export function dropOver(x: number, y: number): boolean {
  const c = pick(x, y)
  if (current && current !== c) current.leave()
  current = c
  c?.over(x, y)
  return !!c
}

/** 文件拖出了窗口，或者被取消了。 */
export function dropLeave() {
  current?.leave()
  current = null
}

/** 松手。有页面接收就交给它并返回 true。 */
export function dropFiles(paths: string[], x: number, y: number): boolean {
  const c = pick(x, y)
  current?.leave()
  current = null
  if (!c) return false
  c.drop(paths, x, y)
  return true
}
