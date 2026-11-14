// 后台的“稍后执行”（去抖 / 延迟任务）：Service Worker 空闲约 30 秒就会被回收，Firefox 事件页同理，
// 内存里的 setTimeout 随之丢失。这里把待办（名字 → 到期时间）存进存储，同时设一个 chrome.alarms 做兜底：
// - 浏览器还活着：用 setTimeout 按时执行（精确到毫秒）；
// - 后台被回收：alarm 到点把后台唤醒（浏览器对 alarm 有最小间隔，约 30 秒），后台启动时 flush() 补做到期的待办。
// 存储用 storage.session（旧 Firefox 没有就退回 storage.local：重启浏览器后残留的待办也会补做，无害）。
// 待办只记名字，不带内存里的数据：处理函数要自己能从存储里把需要的东西读出来。

const ccDefer = (() => {
  const KEY = 'ccDefer'
  const PREFIX = 'cc-defer:'
  const handlers = new Map()
  const timers = new Map()
  let lock = Promise.resolve()

  const store = () => api.storage.session || api.storage.local
  /** 串行执行，避免连续安排 / 取消时读改写互相覆盖。 */
  const serial = (fn) => {
    const p = lock.then(fn)
    lock = p.catch(() => {})
    return p
  }
  async function readAll() {
    try {
      const r = (await store().get(KEY))[KEY]
      return r && typeof r === 'object' ? r : {}
    } catch {
      return {}
    }
  }
  const writeAll = (all) => store().set({ [KEY]: all })

  /** 注册处理函数。名字形如 `类别` 或 `类别:参数`，按类别找处理函数，参数原样传入。 */
  function on(kind, fn) {
    handlers.set(kind, fn)
  }

  function arm(name, due) {
    clearTimeout(timers.get(name))
    timers.set(name, setTimeout(() => fire(name).catch((e) => console.warn('deferred task failed', name, e)), Math.max(0, due - Date.now())))
  }

  /** 安排（或重新安排）一件事：delayMs 毫秒后执行。同名的再次安排会把时间往后推（去抖）。 */
  function schedule(name, delayMs) {
    const due = Date.now() + Math.max(0, delayMs)
    clearTimeout(timers.get(name))
    return serial(async () => {
      const all = await readAll()
      all[name] = due
      try {
        await writeAll(all)
      } catch (e) {
        console.warn('defer persist failed', e)
      }
      // 先存好再挂定时器：定时器到点时 fire 要能在存储里找到这件事
      arm(name, due)
      try {
        await api.alarms?.create(PREFIX + name, { when: Math.max(due, Date.now() + 1000) })
      } catch {
        // 没有 alarms 权限：只靠内存里的定时器
      }
    })
  }

  /** 取消一件还没执行的事。 */
  function cancel(name) {
    clearTimeout(timers.get(name))
    timers.delete(name)
    return serial(async () => {
      const all = await readAll()
      if (name in all) {
        delete all[name]
        await writeAll(all).catch(() => {})
      }
      try {
        await api.alarms?.clear(PREFIX + name)
      } catch {
        // 忽略
      }
    })
  }

  /** 到点了：从待办里取走并执行。被重新安排得更晚的（去抖）、已经取消或做过的不做。 */
  async function fire(name) {
    const due = await serial(async () => {
      const all = await readAll()
      const t = all[name]
      if (typeof t !== 'number') return null
      if (t > Date.now() + 5) return t // 被往后推了：留着，等新的时间
      delete all[name]
      await writeAll(all).catch(() => {})
      return 0
    })
    if (due === null) return
    if (due > 0) {
      if (!timers.has(name)) arm(name, due)
      return
    }
    clearTimeout(timers.get(name))
    timers.delete(name)
    try {
      await api.alarms?.clear(PREFIX + name)
    } catch {
      // 忽略
    }
    const i = name.indexOf(':')
    const fn = handlers.get(i < 0 ? name : name.slice(0, i))
    if (fn) await fn(i < 0 ? '' : name.slice(i + 1))
  }

  /** 后台启动或 alarm 唤醒时调用：到期的马上做，没到期的重新挂上定时器。 */
  async function flush() {
    const all = await serial(readAll)
    for (const [name, due] of Object.entries(all)) {
      if (typeof due !== 'number') continue
      if (due <= Date.now()) await fire(name).catch((e) => console.warn('deferred task failed', name, e))
      else if (!timers.has(name)) arm(name, due)
    }
  }

  if (api.alarms?.onAlarm) {
    api.alarms.onAlarm.addListener((a) => {
      if (a.name.startsWith(PREFIX)) flush().catch((e) => console.warn('defer flush failed', e))
    })
  }

  return { on, schedule, cancel, flush, PREFIX }
})()
