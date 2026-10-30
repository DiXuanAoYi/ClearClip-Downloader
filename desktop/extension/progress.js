// 下载进度：任务汇总、显示文字。纯函数，不依赖浏览器接口。

/** 任务列表（/api/v1/tasks）→ 汇总。 */
function summarizeTasks(tasks) {
  const list = Array.isArray(tasks) ? tasks : []
  const count = (s) => list.filter((t) => t.status === s).length
  const needLogin = list.filter((t) => t.status === 'failed' && t.errorKind === 'need_login').length
  return {
    running: count('running'),
    queued: count('queued'),
    paused: count('paused'),
    failed: count('failed'),
    done: count('done'),
    needLogin,
    speed: list.filter((t) => t.status === 'running').reduce((a, t) => a + (Number(t.speed) || 0), 0),
    active: count('running') + count('queued'),
  }
}

/** 图标角标文字：进行中的任务数，没有就空；超过 99 显示 99+。 */
function badgeFor(summary) {
  const n = summary.active
  return n > 0 ? (n > 99 ? '99+' : String(n)) : ''
}

function fmtBytes(n) {
  const v = Number(n) || 0
  if (v < 1024) return `${v} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let x = v / 1024
  let i = 0
  while (x >= 1024 && i < units.length - 1) {
    x /= 1024
    i++
  }
  return `${x >= 100 ? x.toFixed(0) : x.toFixed(1)} ${units[i]}`
}

function fmtEta(seconds) {
  const s = Math.round(seconds)
  if (!isFinite(s) || s <= 0) return ''
  if (s < 60) return `${s} 秒`
  if (s < 3600) return `${Math.floor(s / 60)} 分 ${s % 60} 秒`
  return `${Math.floor(s / 3600)} 小时 ${Math.floor((s % 3600) / 60)} 分`
}

const TASK_STEP = { merge: '合并分片中', post: '处理中' }

/** 一个任务在弹窗里的显示：{ id, name, state, pct, line, actions }。actions：pause / resume / reveal / login。 */
function describeTask(t) {
  const name = t.fileName || t.title || `任务 ${t.id}`
  const total = Number(t.total) || 0
  const pct = total > 0 ? Math.min(100, Math.floor(((Number(t.received) || 0) * 100) / total)) : null
  const base = { id: t.id, name, pct, state: t.status }
  switch (t.status) {
    case 'running': {
      const speed = Number(t.speed) || 0
      const eta = speed > 0 && total > 0 ? fmtEta((total - (Number(t.received) || 0)) / speed) : ''
      const parts = [pct == null ? fmtBytes(t.received) : `${pct}%`, speed ? `${fmtBytes(speed)}/s` : '', TASK_STEP[t.step] || (eta ? `剩余 ${eta}` : '')]
      return { ...base, line: parts.filter(Boolean).join(' · '), actions: ['pause', 'reveal'] }
    }
    case 'queued':
      return { ...base, line: '排队中', actions: ['pause'] }
    case 'paused':
      return { ...base, line: pct == null ? '已暂停' : `已暂停 · ${pct}%`, actions: ['resume', 'reveal'] }
    case 'done':
      return { ...base, pct: 100, line: `已完成${total ? ` · ${fmtBytes(total)}` : ''}`, actions: ['reveal'] }
    case 'failed':
      return t.errorKind === 'need_login'
        ? { ...base, line: '需要登录', actions: ['login', 'resume'] }
        : { ...base, line: t.error ? String(t.error).slice(0, 60) : '下载失败', actions: ['resume'] }
    default:
      return { ...base, line: '已取消', actions: [] }
  }
}

/** 弹窗里的显示顺序：进行中、排队、暂停、失败，最后是最近完成的；最多 30 个。 */
function arrangeTasks(tasks) {
  const rank = { running: 0, queued: 1, paused: 2, failed: 3, done: 4, canceled: 5 }
  return [...(tasks || [])]
    .filter((t) => t.status !== 'canceled')
    .sort((a, b) => (rank[a.status] ?? 9) - (rank[b.status] ?? 9) || (b.createdAt || 0) - (a.createdAt || 0))
    .slice(0, 30)
}

/** 新完成的任务编号（上次没完成、这次完成了的）：用来发完成通知。 */
function newlyDone(prevDoneIds, tasks) {
  const prev = new Set(prevDoneIds || [])
  return (tasks || []).filter((t) => t.status === 'done' && !prev.has(t.id))
}
