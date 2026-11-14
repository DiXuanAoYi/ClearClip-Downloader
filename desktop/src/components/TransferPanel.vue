<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import type { ChatMsg, PhoneInfo, TransferItem, TransferList } from '../types'
import { formatBytes, formatDateTime } from '../utils/format'
import { registerDrop } from '../utils/dropHub'
import { useAppStore } from '../stores/app'
import { useTauriEvents } from '../composables/useTauriEvent'
const ev = useTauriEvents()

const app = useAppStore()
const list = ref<TransferList>({ incoming: [], outgoing: [], receiveDir: '' })
const info = ref<PhoneInfo | null>(null)
const text = ref('')
const over = ref(false)
const sending = ref(false)
/** 发送对象：勾选的设备编号。默认所有手机 / 平板，不含浏览器扩展（编号 ext- 开头：它收到的内容会再交回主程序，造成来回传） */
const targets = ref<Set<string>>(new Set())
const isExt = (id: string) => id.startsWith('ext-')
const seen = new Set<string>()
watch(
  () => info.value?.devices.map((d) => d.id).join(','),
  () => {
    const ids = info.value?.devices.map((d) => d.id) ?? []
    const keep = new Set([...targets.value].filter((id) => ids.includes(id)))
    for (const id of ids) if (!isExt(id) && !seen.has(id)) keep.add(id)
    ids.forEach((id) => seen.add(id))
    targets.value = keep
  },
  { immediate: true },
)
function toggleTarget(id: string) {
  const s = new Set(targets.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  targets.value = s
}
const targetText = computed(() => {
  const names = (info.value?.devices ?? []).filter((d) => targets.value.has(d.id)).map((d) => d.name)
  return names.length ? `发给 ${names.join('、')}` : '没有选发送对象'
})
const targetList = () => [...targets.value]
/** 图片缩略图：向后端要（JPEG / PNG / WebP 才有；HEIC 等显示文件图标），按路径缓存 */
const thumbs = ref<Record<string, string>>({})
const THUMB_EXT = /\.(jpe?g|png|webp)$/i
async function loadThumbs() {
  for (const it of [...list.value.incoming, ...list.value.outgoing]) {
    if (it.kind !== 'image' || !it.path || it.missing || it.status === 'receiving' || !THUMB_EXT.test(it.name) || it.path in thumbs.value) continue
    thumbs.value[it.path] = ''
    api
      .transferThumb(it.path)
      .then((u) => u && (thumbs.value[it.path] = u))
      .catch(() => {}) // 忽略：缩略图是可选的
  }
}
let unlisten: UnlistenFn[] = []
let unregister: (() => void) | undefined
let reloadTimer: number | undefined


const hasPeer = computed(() => !!info.value?.devices.length)

/** 传输流：收到的和发出的按时间排在一起（旧的在上，新的在下） */
const stream = computed(() => [...list.value.incoming, ...list.value.outgoing].sort((a, b) => a.createdAt - b.createdAt || a.id - b.id))
const feedEl = ref<HTMLElement | null>(null)
/** 在线：最近 2 分钟内这台设备用过 */
const online = (lastSeen: number) => Math.floor(Date.now() / 1000) - lastSeen < 120
const ICONS: Record<string, string> = {
  image: '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="m21 16-5-5-8 9"/>',
  video: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m10 9 5 3-5 3z"/>',
  audio: '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  archive: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5M10 9v2M10 13v2"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  text: '<path d="M4 6h16M4 12h16M4 18h10"/>',
}
const COLORS: Record<string, string> = { image: '#c98a3b', video: '#4b7bb5', audio: '#7a8f6a', archive: '#8a6d3b', file: '#b45f00', text: '#7a62b5' }
const iconOf = (k: string) => ICONS[k] ?? ICONS.file
const colorOf = (k: string) => COLORS[k] ?? COLORS.file
/** 日期分隔：同一天只显示一次 */
function dayLabel(ts: number): string {
  const d = new Date(ts * 1000)
  const now = new Date()
  const same = d.toDateString() === now.toDateString()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  return same ? `今天 ${hm}` : `${d.getMonth() + 1} 月 ${d.getDate()} 日 ${hm}`
}
const rows = computed(() => {
  let last = ''
  return stream.value.map((it) => {
    const key = new Date(it.createdAt * 1000).toDateString() + Math.floor(it.createdAt / 1800)
    const day = key !== last ? dayLabel(it.createdAt) : ''
    last = key
    return { it, day }
  })
})
watch(
  () => stream.value.length,
  async () => {
    await nextTick()
    if (feedEl.value) feedEl.value.scrollTop = feedEl.value.scrollHeight
  },
)
async function revoke(id: string, name: string) {
  try {
    await ElMessageBox.confirm(`撤销“${name}”后，它需要重新配对才能再发送。`, '撤销设备', { type: 'warning', confirmButtonText: '撤销', cancelButtonText: '取消' })
    info.value = await api.phoneRevoke(id)
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error(errorText(e))
  }
}
async function pickRecvDir() {
  const r = await open({ directory: true })
  if (typeof r === 'string' && info.value) {
    try {
      await api.phoneSetOptions({ host: info.value.host, receiveDir: r, maxUploadMb: info.value.maxUploadMb, autoExtract: info.value.autoExtract, outboxHours: info.value.outboxHours })
      await Promise.all([load(), loadInfo()])
    } catch (e) {
      ElMessage.error(errorText(e))
    }
  }
}

async function load() {
  try {
    list.value = await api.transferList()
    loadThumbs()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function answerPair(deviceId: string, accept: boolean) {
  try {
    await api.phonePairRespond(deviceId, accept)
    if (accept) ElMessage.success('已配对')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    await loadInfo()
  }
}

async function loadInfo() {
  try {
    info.value = await api.phoneInfo()
  } catch {
    /* 面板其余部分仍可使用 */
  }
}

/** 变化事件可能很密集（多个文件同时到达），合并成一次刷新。 */
function scheduleLoad() {
  if (reloadTimer) return
  reloadTimer = window.setTimeout(() => {
    reloadTimer = undefined
    load()
  }, 120)
}

function onProgress(id: number, done: number) {
  for (const it of [...list.value.incoming, ...list.value.outgoing]) {
    if (it.id === id) {
      it.done = Math.max(it.done, done)
      it.active = true
    }
  }
}

async function enable() {
  try {
    info.value = await api.phoneEnable(true)
    if (info.value.running) ElMessage.success('已开启。第一次开启时系统可能弹出防火墙提示，请允许“专用网络”访问。')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

/**
 * 微信式输入框：粘贴 / 选择 / 拖入的文件先“放进输入框”（草稿），点“发送”才发出，不会自动发。
 * 草稿是按放入顺序排的一串：文字和文件交错时，先打的文字会在放文件的那一刻固定成一条，
 * 发送时严格按这个顺序发（输入框里还没固定的文字排在最后）。
 */
type Seg = { id: number; kind: 'text'; text: string } | { id: number; kind: 'file'; path: string; name: string }
const draft = ref<Seg[]>([])
let segSeq = 0
const baseName = (p: string) => p.split(/[\\/]/).pop() || p
const canSend = computed(() => (draft.value.length > 0 || !!text.value.trim()) && targets.value.size > 0)
/** 把输入框里正在打的文字固定成草稿里的一条（保持顺序） */
function commitText() {
  const t = text.value.trim()
  if (t) draft.value.push({ id: ++segSeq, kind: 'text', text: t })
  text.value = ''
}
function stagePaths(paths: string[]) {
  if (!paths.length) return
  commitText()
  for (const path of paths.slice(0, 50)) {
    draft.value.push({ id: ++segSeq, kind: 'file', path, name: baseName(path) })
    void ensureThumb(path)
  }
}
const isImageName = (n: string) => THUMB_EXT.test(n)
async function ensureThumb(path: string) {
  if (!THUMB_EXT.test(path) || path in thumbs.value) return
  thumbs.value[path] = ''
  try {
    const u = await api.transferThumb(path)
    if (u) thumbs.value[path] = u
  } catch {
    /* 没有缩略图就显示文件图标 */
  }
}
/** 输入框是空的时候按退格：删掉草稿里最后一项（和微信一样） */
function onBackspace(e: KeyboardEvent) {
  if (text.value === '' && draft.value.length && !e.isComposing) {
    e.preventDefault()
    draft.value.pop()
  }
}
function dropSeg(id: number) {
  draft.value = draft.value.filter((x) => x.id !== id)
}

async function pick() {
  const r = await open({ multiple: true })
  if (!r) return
  stagePaths(Array.isArray(r) ? r : [r])
}

/** 剪贴板里的图片 / 文件（截图、在资源管理器里复制的文件）：存成临时文件，放进输入框（不发送） */
async function pasteFiles(files: File[]) {
  if (!files.length) return
  sending.value = true
  try {
    const stamp = () => {
      const d = new Date()
      const z = (n: number) => String(n).padStart(2, '0')
      return `${d.getFullYear()}${z(d.getMonth() + 1)}${z(d.getDate())}-${z(d.getHours())}${z(d.getMinutes())}${z(d.getSeconds())}`
    }
    const paths: string[] = []
    let i = 0
    for (const f of files.slice(0, 20)) {
      // 截图粘贴过来叫 image.png，换成带时间的名字
      const generic = /^image\.(png|jpe?g|gif|webp|bmp)$/i.test(f.name)
      const ext = f.name.includes('.') ? f.name.slice(f.name.lastIndexOf('.')) : f.type === 'image/png' ? '.png' : ''
      const name = generic || !f.name ? `粘贴的图片-${stamp()}${i ? '-' + i : ''}${ext || '.png'}` : f.name
      i++
      paths.push(await api.transferStageFile(name, await f.arrayBuffer()))
    }
    stagePaths(paths)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    sending.value = false
  }
}

function onPaste(e: ClipboardEvent) {
  const files = [...(e.clipboardData?.files ?? [])]
  if (!files.length) return // 只有文字：交给输入框自己粘贴
  e.preventDefault()
  void pasteFiles(files)
}

/** 发送整个草稿：按顺序一条一条发；输入框里没固定的文字排在最后。 */
async function sendAll() {
  if (!canSend.value || sending.value) return
  commitText()
  // 只发点击这一刻的草稿：发送过程中新放进来的，要等下一次点发送
  const batchAll = draft.value.splice(0)
  sending.value = true
  try {
    let sentText = 0
    let sentFiles = 0
    // 连续的文件合成一批发，文字单独发；批与批之间按顺序等前一个完成
    while (batchAll.length) {
      const first = batchAll[0]
      if (first.kind === 'text') {
        await api.transferSendText(first.text, targetList())
        sentText++
        batchAll.shift()
      } else {
        const batch: string[] = []
        let n = 0
        while (n < batchAll.length && batchAll[n].kind === 'file') {
          batch.push((batchAll[n] as Extract<Seg, { kind: 'file' }>).path)
          n++
        }
        const r = await api.transferSendFiles(batch, targetList())
        sentFiles += r.added
        for (const w of r.skipped.slice(0, 3)) ElMessage.warning(w)
        batchAll.splice(0, n)
      }
    }
    ElMessage.success(sentFiles ? `已发送，手机网页的“电脑发来的”里可以下载${sentText ? '、复制' : ''}` : '已加入，手机网页里可以一键复制')
    await load()
  } catch (e) {
    // 已发出的已经去掉，没发的放回草稿最前面，改好再点发送
    draft.value.unshift(...batchAll)
    ElMessage.error(errorText(e))
  } finally {
    sending.value = false
  }
}

async function remove(it: TransferItem) {
  let deleteFile = false
  if (it.direction === 'in' && it.status === 'done' && it.path && !it.missing) {
    try {
      await ElMessageBox.confirm(`只移除这条记录，还是同时删除电脑上收到的文件“${it.name}”？`, '删除', {
        type: 'warning',
        distinguishCancelAndClose: true,
        confirmButtonText: '同时删除文件',
        cancelButtonText: '只移除记录',
      })
      deleteFile = true
    } catch (e) {
      if (e === 'close') return
    }
  }
  try {
    await api.transferRemove(it.id, deleteFile)
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function clearAll() {
  await api.transferClear('in')
  await api.transferClear('out')
  await load()
}

async function copy(t: string) {
  await api.copyText(t)
  ElMessage.success('已复制')
}

async function reveal(path: string) {
  try {
    await api.revealFile(path)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function openIt(path: string) {
  try {
    await api.openFile(path)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

const pct = (it: TransferItem) => (it.size > 0 ? Math.min(100, Math.floor((it.done * 100) / it.size)) : it.status === 'done' ? 100 : 0)

function expiry(it: TransferItem): string {
  if (!it.expiresAt) return '不过期'
  const left = it.expiresAt - Math.floor(Date.now() / 1000)
  if (left <= 0) return '已过期'
  return left >= 3600 ? `${Math.ceil(left / 3600)} 小时后撤回` : `${Math.max(1, Math.ceil(left / 60))} 分钟后撤回`
}

/** 手机互聊：手机之间的消息经电脑上的服务转发，开关打开时这里能看到全部内容（只读） */
const view = ref<'files' | 'chat'>('files')
const chat = ref<ChatMsg[]>([])
const chatEl = ref<HTMLElement | null>(null)
const chatOn = computed(() => !!info.value?.peerChat)
async function setChat(on: boolean) {
  if (!info.value) return
  try {
    const i = info.value
    info.value = await api.phoneSetOptions({ host: i.host, receiveDir: i.receiveDir, maxUploadMb: i.maxUploadMb, autoExtract: i.autoExtract, outboxHours: i.outboxHours, peerChat: on })
    if (on) {
      chat.value = await api.phoneChatList()
      view.value = 'chat'
    } else {
      chat.value = []
      view.value = 'files'
    }
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function clearChat() {
  try {
    await ElMessageBox.confirm('清空后，电脑上看不到之前的聊天内容；手机网页里的记录也会消失。', '清空手机互聊记录', { confirmButtonText: '清空', cancelButtonText: '取消', type: 'warning' })
    await api.phoneChatClear()
  } catch (e) {
    if (e !== 'cancel' && e !== 'close') ElMessage.error(errorText(e))
  }
}
function onChatMsg(m: ChatMsg & { cleared?: boolean }) {
  if (m.cleared) {
    chat.value = []
    return
  }
  if (!chat.value.some((x) => x.id === m.id)) chat.value.push(m)
}
const chatRows = computed(() => {
  let last = ''
  return chat.value.map((m) => {
    const key = new Date(m.ts * 1000).toDateString() + Math.floor(m.ts / 1800)
    const day = key !== last ? dayLabel(m.ts) : ''
    last = key
    return { m, day }
  })
})
watch(
  () => chat.value.length,
  async () => {
    await nextTick()
    if (chatEl.value) chatEl.value.scrollTop = chatEl.value.scrollHeight
  },
)

onMounted(async () => {
  await Promise.all([load(), loadInfo()])
  if (info.value?.peerChat) chat.value = await api.phoneChatList().catch(() => []) // 忽略：可选数据，取不到就按空处理
  unlisten = [
    await ev.on(events.onChat(onChatMsg)),
    await ev.on(events.onTransferChanged(scheduleLoad)),
    await ev.on(events.onTransferProgress((p) => onProgress(p.id, p.done))),
    await ev.on(events.onPairRequest(loadInfo)),
  ]
  // 整个窗口的文件拖放事件只有一个入口：这个页面可见时，拖进来的文件都是要发给手机的
  unregister = registerDrop({
    accepts: () => true,
    over: () => (over.value = true),
    leave: () => (over.value = false),
    drop: (paths) => {
      over.value = false
      stagePaths(paths)
    },
  })
})
onUnmounted(() => {
  unlisten.forEach((u) => u())
  unregister?.()
  window.clearTimeout(reloadTimer)
})
</script>

<template>
  <div class="xfer" :class="{ over }">
    <div class="topbar">
      <b class="ttl">手机互传</b>
      <span v-if="info?.running && info.url" class="pill ok"><i />服务已开启 · {{ info.ip }}:{{ info.port }}</span>
      <span v-else class="pill off"><i />服务未开启</span>
      <span class="spacer" />
      <span class="mute small hide-sm">只在局域网内传输，不经过任何服务器</span>
      <el-button size="small" @click="app.goSettings('phone')">选项</el-button>
    </div>

    <el-alert v-if="info && !info.enabled" class="alert" type="info" :closable="false" show-icon title="手机互传还没有开启" description="开启后，手机扫码就能把链接、文字、照片、视频和文件发到电脑，也能下载电脑发给手机的内容。">
      <template #default><el-button size="small" type="primary" @click="enable">开启</el-button></template>
    </el-alert>
    <el-alert v-else-if="info && info.error" class="alert" type="warning" :closable="false" show-icon :title="info.error" />

    <div class="body">
      <aside class="side">
        <div class="box qrbox">
          <b>手机扫码连接</b>
          <div v-if="info?.running && info.url" class="qr" v-html="info.qrSvg" />
          <div v-else class="qr off mute">开启服务后<br>这里显示二维码</div>
          <small class="mute center">用手机相机或微信扫一扫<br>手机和电脑需连同一个 Wi-Fi{{ hasPeer ? '' : '；第一次扫码时电脑上会请你确认' }}</small>
          <small v-if="info?.suspicious" class="warn">没有找到常见的 Wi-Fi / 有线网卡，当前地址可能是代理软件的虚拟网卡。</small>
          <div class="row">
            <el-button size="small" :disabled="!info?.url" @click="copy(info!.url!)">复制网址</el-button>
            <el-button size="small" @click="app.goSettings('phone')">换个地址</el-button>
          </div>
        </div>

        <div class="devs">
          <div class="dhead"><b>发送对象</b><span class="mute small">已选 {{ targets.size }} / {{ info?.devices.length ?? 0 }}</span></div>
          <div v-for="p in info?.pending ?? []" :key="p.deviceId" class="dev pend">
            <span class="avatar"><svg viewBox="0 0 24 24"><rect x="7" y="2" width="10" height="20" rx="2.5" /><path d="M11 18h2" /></svg></span>
            <span class="dinfo">
              <b class="ellipsis">{{ p.name }}</b>
              <small class="mute">请求配对<template v-if="p.code"> · 验证码 <b class="code">{{ p.code }}</b></template></small>
            </span>
            <el-button size="small" type="primary" @click="answerPair(p.deviceId, true)">允许</el-button>
            <a class="link mutelink" @click="answerPair(p.deviceId, false)">拒绝</a>
          </div>
          <div v-if="!info?.devices.length && !info?.pending?.length" class="mute small">
            还没有配对的设备。手机扫码后会出现在这里；如果扫码时手机还没连上同一个 Wi-Fi，连上之后在手机上刷新这个页面，这里就会出现配对请求。
          </div>
          <div v-for="d in info?.devices" :key="d.id" class="dev" :class="{ ext: isExt(d.id) }">
            <el-checkbox :model-value="targets.has(d.id)" :aria-label="`发给${d.name}`" @change="toggleTarget(d.id)" />
            <span class="avatar" :class="{ ext: isExt(d.id) }">
              <svg v-if="!isExt(d.id)" viewBox="0 0 24 24"><rect x="7" y="2" width="10" height="20" rx="2.5" /><path d="M11 18h2" /></svg>
              <svg v-else viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
            </span>
            <span class="dinfo">
              <b class="ellipsis">{{ d.name }}</b>
              <small class="mute"><span v-if="online(d.lastSeen)" class="on">● 在线</span><template v-else>最近 {{ formatDateTime(d.lastSeen) }}</template><template v-if="isExt(d.id)"> · 浏览器扩展</template></small>
            </span>
            <a class="link mutelink" @click="revoke(d.id, d.name)">撤销</a>
          </div>
          <small v-if="info?.devices.length" class="mute hint2">勾选的设备才会收到你发出的文件和文字。浏览器扩展默认不勾：它收到的内容会再交回主程序，造成来回传。</small>
        </div>

        <div class="box chatbox">
          <div class="chead">
            <b>手机互聊</b>
            <el-switch :model-value="chatOn" size="small" :disabled="!info?.running" aria-label="手机互聊" data-testid="chat-switch" @change="(v: string | number | boolean) => setChat(!!v)" />
          </div>
          <small class="mute">开启后，已配对的手机能在网页里互相发文字，不用经过你操作。消息只在局域网内流转，但要经过这台电脑上的清影转发，所以你在这里能看到全部内容。关闭或退出清影后记录清空。</small>
        </div>

        <div class="box dirbox">
          <small class="mute">收到的文件保存在</small>
          <b class="dir selectable">{{ list.receiveDir || info?.receiveDir }}</b>
          <div class="row">
            <a class="link" @click="reveal(list.receiveDir || info!.receiveDir)">打开文件夹</a>
            <a class="link" @click="pickRecvDir">更改…</a>
          </div>
        </div>
      </aside>

      <section class="main">
        <div v-if="chatOn" class="vtabs" role="tablist">
          <button role="tab" :aria-selected="view === 'files'" :class="{ on: view === 'files' }" @click="view = 'files'">传输记录</button>
          <button role="tab" :aria-selected="view === 'chat'" :class="{ on: view === 'chat' }" data-testid="chat-tab" @click="view = 'chat'">手机互聊<em v-if="chat.length">{{ chat.length }}</em></button>
        </div>
        <div v-if="chatOn && view === 'chat'" ref="chatEl" class="feed chatfeed" data-testid="chat-feed">
          <div v-if="!chat.length" class="empty mute">
            <b>还没有聊天内容</b>
            <span>手机网页里会多一个“手机互聊”页，扫码连上的手机在那里发的消息会实时出现在这里。</span>
          </div>
          <template v-for="r in chatRows" :key="r.m.id">
            <div v-if="r.day" class="day">{{ r.day }}</div>
            <div class="cmsg">
              <span class="avatar"><svg viewBox="0 0 24 24"><rect x="7" y="2" width="10" height="20" rx="2.5" /><path d="M11 18h2" /></svg></span>
              <div class="cbody">
                <div class="cwho">{{ r.m.name || '手机' }}</div>
                <div class="cbub selectable">{{ r.m.text }}</div>
              </div>
            </div>
          </template>
        </div>
        <div v-if="chatOn && view === 'chat'" class="ctail">
          <small class="mute">只读：这里显示手机之间的对话，不会发给手机。</small>
          <a v-if="chat.length" class="link" @click="clearChat">清空聊天记录</a>
        </div>
        <div v-show="!(chatOn && view === 'chat')" ref="feedEl" class="feed">
          <div v-if="!rows.length" class="empty mute">
            <b>还没有传输记录</b>
            <span>手机扫码后，发来的文件、图片、链接和文字会出现在这里；<br>把文件拖进窗口，或点下面的“选择文件”，就能发给手机。</span>
          </div>
          <template v-for="r in rows" :key="r.it.id">
            <div v-if="r.day" class="day">{{ r.day }}</div>
            <div class="msg" :class="r.it.direction">
              <div class="who">{{ r.it.direction === 'in' ? r.it.deviceName || '手机' : `发给 ${r.it.deviceName || '所有设备'}` }}</div>
              <div class="bub">
                <div v-if="r.it.kind === 'text'" class="txt selectable">{{ r.it.text }}</div>
                <img v-if="thumbs[r.it.path]" class="pic" :src="thumbs[r.it.path]" alt="" @click="openIt(r.it.path)" />
                <div v-if="r.it.kind !== 'text'" class="line">
                  <span v-if="!thumbs[r.it.path]" class="thumb" :style="{ background: colorOf(r.it.kind) }"><svg viewBox="0 0 24 24" v-html="iconOf(r.it.kind)" /></span>
                  <span class="meta">
                    <b class="ellipsis" :title="r.it.path">{{ r.it.name }}</b>
                    <small class="mute">
                      {{ formatBytes(r.it.size) }} · 
                      <template v-if="r.it.direction === 'in'">
                        <span v-if="r.it.status === 'receiving'">接收中 {{ pct(r.it) }}%</span>
                        <span v-else-if="r.it.status === 'failed'" class="bad">失败：{{ r.it.message }}</span>
                        <span v-else-if="r.it.missing" class="bad">文件已被移动或删除</span>
                        <span v-else class="ok">已保存</span>
                      </template>
                      <template v-else>
                        <span v-if="r.it.missing" class="bad">原文件已被移动或删除</span>
                        <span v-else-if="r.it.active">手机正在下载</span>
                        <span v-else>手机已下载 {{ r.it.downloads }} 次</span>
                        · {{ expiry(r.it) }}
                      </template>
                    </small>
                  </span>
                </div>
                <el-progress v-if="r.it.status === 'receiving' || (r.it.direction === 'out' && r.it.active)" :percentage="pct(r.it)" :stroke-width="5" :show-text="false" />
                <small v-if="r.it.extra" class="mute">解压到 <a class="link" @click="reveal(r.it.extra!)">{{ r.it.extra }}</a></small>
                <div class="brow">
                <small v-if="r.it.kind === 'text'" class="mute kind">文字{{ r.it.direction === 'out' ? ' · 手机上可一键复制' : '' }}</small>
                <div class="acts">
                  <template v-if="r.it.direction === 'in' && r.it.status === 'done' && r.it.kind === 'text'">
                    <a class="link" @click="copy(r.it.text)">复制</a>
                  </template>
                  <template v-else-if="r.it.direction === 'in' && r.it.status === 'done' && !r.it.missing">
                    <a class="link" @click="openIt(r.it.path)">打开</a>
                    <a class="link" @click="reveal(r.it.path)">显示位置</a>
                  </template>
                  <a v-if="r.it.kind === 'text' && r.it.direction === 'out'" class="link" @click="copy(r.it.text)">复制</a>
                  <a class="link" @click="remove(r.it)">{{ r.it.direction === 'out' ? '撤回' : r.it.status === 'receiving' ? '取消' : '删除' }}</a>
                </div>
                </div>
              </div>
            </div>
          </template>
        </div>

        <div v-if="over && !(chatOn && view === 'chat')" class="dropmask"><svg viewBox="0 0 24 24"><path d="M12 16V4M7 9l5-5 5 5" /><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3" /></svg>松开鼠标，放进输入框（点“发送”才会发给 {{ targetText.replace('发给 ', '') }}）</div>
        <div v-show="!(chatOn && view === 'chat')" class="composer">
          <div v-if="draft.length" class="tray" data-testid="draft" aria-label="待发送的内容（按这个顺序发出）">
            <div v-for="(d, n) in draft" :key="d.id" class="dchip" :class="d.kind" :data-kind="d.kind">
              <span class="no">{{ n + 1 }}</span>
              <template v-if="d.kind === 'text'"><span class="dtext" :title="d.text">{{ d.text }}</span></template>
              <template v-else>
                <img v-if="thumbs[d.path]" class="dthumb" :src="thumbs[d.path]" alt="" />
                <span v-else class="dicon"><svg viewBox="0 0 24 24" v-html="iconOf(isImageName(d.name) ? 'image' : 'file')" /></span>
                <span class="dtext" :title="d.path">{{ d.name }}</span>
              </template>
              <button type="button" class="dx" :aria-label="`移除第 ${n + 1} 项`" @click="dropSeg(d.id)">×</button>
            </div>
          </div>
          <div class="cbar">
            <span class="tchip" :class="{ none: !targets.size }" :title="'在左侧“发送对象”里勾选'">{{ targetText }}</span>
            <el-button class="clip" text :loading="sending" aria-label="选择文件" title="选择文件…" @click="pick">
              <svg viewBox="0 0 24 24"><path d="m21 12-8.5 8.5a5 5 0 0 1-7-7L14 5a3.3 3.3 0 0 1 4.7 4.7L10 18.4a1.7 1.7 0 0 1-2.4-2.4L15 8.6" /></svg>
            </el-button>
            <input v-model="text" class="tin" type="text" maxlength="20000" placeholder="输入文字，或把文件拖进窗口，或直接粘贴图片 / 文件（点“发送”才会发出）…" @keydown.enter.exact.prevent="sendAll" @keydown.backspace="onBackspace" @paste="onPaste" />
            <el-button type="primary" class="sendb" :disabled="!canSend" :loading="sending" @click="sendAll">发送{{ draft.length ? `（${draft.length + (text.trim() ? 1 : 0)}）` : '' }}</el-button>
          </div>
          <div class="tail">
            <small class="mute">收到的文件不会被自动运行，打开前请确认来源可信。</small>
            <a v-if="list.incoming.some((i) => i.status !== 'receiving') || list.outgoing.some((i) => !i.active)" class="link" @click="clearAll">清空记录</a>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.xfer {
  --x-acc: #b45f00;
  --x-acfg: #fff;
  --x-ob: #fbf1e2;
  --x-obl: #e3b475;
  --x-obfg: #3a2500;
  --x-obmt: #8a5a1a;
  --x-ok: #007a70;
  --x-chip: #e4ebf5;
  --x-chipfg: #0b1424;
}
:global(html.dark .xfer) {
  --x-acc: #ffb547;
  --x-acfg: #1a1100;
  --x-ob: #2a2016;
  --x-obl: #8a6328;
  --x-obfg: #ffe9c4;
  --x-obmt: #d7b793;
  --x-ok: #4be3d0;
  --x-chip: #131f33;
  --x-chipfg: #e8f0fd;
}
.xfer {
    display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  gap: 0;
  border: 1px solid var(--cc-line);
  border-radius: 14px;
  overflow: hidden;
  background: var(--cc-card);
}
.topbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 22px;
  border-bottom: 1px solid var(--cc-line);
}
.ttl {
  font-size: 17px;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border-radius: 999px;
  padding: 5px 12px;
  font-size: 12.5px;
  font-weight: 600;
}
.pill i {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
}
.pill.ok {
  background: color-mix(in srgb, #4be3d0 14%, transparent);
  color: var(--x-ok);
}
.pill.off {
  background: color-mix(in srgb, var(--cc-mute) 16%, transparent);
  color: var(--cc-mute);
}
.spacer {
  flex: 1;
}
.small {
  font-size: 12.5px;
}
.alert {
  margin: 12px 22px 0;
  width: auto;
}
.body {
  flex: 1;
  min-height: 0;
  display: flex;
}
.side {
  width: 292px;
  flex: none;
  padding: 18px 18px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  border-right: 1px solid var(--cc-line);
  overflow: auto;
}
.box {
  border: 1px solid var(--cc-line);
  border-radius: 16px;
  padding: 14px;
  background: var(--cc-inset);
}
.qrbox {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}
.qr {
  width: 176px;
  height: 176px;
  background: #fff;
  border-radius: 12px;
  padding: 10px;
  box-sizing: border-box;
  border: 1px solid var(--cc-line);
}
.qr :deep(svg) {
  width: 100%;
  height: 100%;
}
.qr.off {
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  font-size: 12.5px;
  line-height: 1.6;
  background: transparent;
  border-style: dashed;
}
.center {
  text-align: center;
  line-height: 1.55;
}
.row {
  display: flex;
  gap: 8px;
}
.warn {
  color: var(--cc-amber);
  line-height: 1.5;
}
.devs {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.dhead {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.dev {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 1px solid var(--cc-line);
  border-radius: 14px;
  padding: 9px 12px;
}
.avatar.ext {
  background: #3f6fa8;
}
.dev.ext {
  border-style: dashed;
}
.hint2 {
  line-height: 1.5;
  font-size: 11.5px;
}
.mutelink {
  color: var(--cc-mute);
}
.avatar {
  width: 34px;
  height: 34px;
  border-radius: 10px;
  background: #2a2622;
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  flex: none;
}
.avatar svg,
.thumb svg {
  width: 20px;
  height: 20px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.dinfo {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.on {
  color: var(--x-ok);
}
.dirbox {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.dir {
  font-size: 12.5px;
  word-break: break-all;
}
.dirbox .row {
  gap: 16px;
}
.link {
  cursor: pointer;
  color: var(--x-acc);
  font-size: 12px;
  font-weight: 600;
  word-break: break-all;
}
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--cc-hover);
}
.feed {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 20px 28px 8px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.empty {
  margin: auto;
  text-align: center;
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 13px;
  line-height: 1.7;
}
.day {
  align-self: center;
  font-size: 11.5px;
  color: var(--cc-mute);
  background: var(--x-chip);
  border-radius: 999px;
  padding: 3px 12px;
}
.msg {
  display: flex;
  flex-direction: column;
  gap: 4px;
  align-items: flex-start;
}
.msg.out {
  align-items: flex-end;
}
.who {
  font-size: 11.5px;
  color: var(--cc-mute);
  padding: 0 4px;
}
.bub {
  width: 400px;
  max-width: 100%;
  box-sizing: border-box;
  border-radius: 16px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--cc-card);
  border: 1px solid var(--cc-line);
  border-top-left-radius: 4px;
}
.msg.out .bub {
  background: var(--x-ob);
  border-color: var(--x-obl);
  border-top-left-radius: 16px;
  border-top-right-radius: 4px;
  color: var(--x-obfg);
}
.msg.out .mute,
.msg.out .who {
  color: var(--x-obmt);
}
.msg.out .who {
  color: var(--cc-mute);
}
.line {
  display: flex;
  gap: 12px;
  align-items: center;
}
.thumb {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  flex: none;
}
.thumb svg {
  width: 22px;
  height: 22px;
}
.meta {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.meta b {
  font-size: 13.5px;
}
.txt {
  font-size: 13.5px;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 10em;
  overflow: auto;
}
.acts {
  display: flex;
  gap: 16px;
}
.ok {
  color: var(--x-ok);
}
.bad {
  color: var(--cc-err);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tail {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
@media (max-width: 860px) {
  .body {
    flex-direction: column;
  }
  .side {
    width: auto;
    border-right: 0;
    border-bottom: 1px solid var(--cc-line);
    max-height: 40%;
  }
  .hide-sm {
    display: none;
  }
}
.pic {
  display: block;
  width: 100%;
  max-height: 180px;
  object-fit: cover;
  border-radius: 11px;
  cursor: pointer;
  background: var(--x-chip);
}
.composer {
  padding: 8px 28px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.cbar {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 44px;
  padding: 0 6px 0 8px;
  border: 1px solid var(--cc-line);
  border-radius: 14px;
  background: var(--cc-card);
}
.tchip {
  flex: none;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  height: 28px;
  line-height: 28px;
  padding: 0 12px;
  border-radius: 999px;
  background: var(--x-chip);
  color: var(--x-chipfg);
  font-size: 12px;
  font-weight: 600;
}
.tchip.none {
  color: var(--cc-err);
}
.clip {
  flex: none;
  padding: 0 8px;
}
.clip svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.tin {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--cc-fg);
  font-size: 13.5px;
}
.tin::placeholder {
  color: var(--cc-mute);
}
.sendb {
  flex: none;
  border-radius: 9px;
  margin-left: 0;
  /* 纯度高的橙色底 + 黑字，浅色、深色下都清楚 */
  --el-button-bg-color: #ff7a14;
  --el-button-border-color: #ff7a14;
  --el-button-text-color: #1a1208;
  --el-button-hover-bg-color: #ff8c33;
  --el-button-hover-border-color: #ff8c33;
  --el-button-hover-text-color: #1a1208;
  --el-button-active-bg-color: #e86a08;
  --el-button-active-border-color: #e86a08;
  --el-button-disabled-bg-color: color-mix(in srgb, #ff7a14 38%, var(--cc-card));
  --el-button-disabled-border-color: color-mix(in srgb, #ff7a14 38%, var(--cc-card));
  --el-button-disabled-text-color: color-mix(in srgb, #1a1208 70%, var(--cc-card));
  font-weight: 700;
}
.dev.pend {
  border-color: var(--x-acc);
  background: color-mix(in srgb, var(--x-acc) 10%, var(--cc-card));
}
.code {
  font-family: var(--el-font-family-mono, monospace);
  letter-spacing: 1px;
  color: var(--cc-fg);
}
.main {
  position: relative;
}
.chatbox {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.chead {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.vtabs {
  display: flex;
  gap: 4px;
  padding: 8px 22px 0;
  border-bottom: 1px solid var(--cc-line);
  background: var(--cc-card);
}
.vtabs button {
  border: 0;
  background: none;
  padding: 8px 14px 10px;
  font: inherit;
  font-weight: 600;
  color: var(--cc-text-2, inherit);
  cursor: pointer;
  border-bottom: 3px solid transparent;
}
.vtabs button.on {
  color: var(--x-acc);
  border-bottom-color: var(--x-acc);
}
.vtabs em {
  font-style: normal;
  margin-left: 6px;
  font-size: 11.5px;
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--x-chip);
  color: var(--x-chipfg);
}
.cmsg {
  display: flex;
  gap: 10px;
  align-items: flex-start;
}
.cbody {
  min-width: 0;
  max-width: min(560px, 80%);
}
.cwho {
  font-size: 12px;
  color: var(--cc-muted, #7a8aa5);
  margin-bottom: 3px;
}
.cbub {
  background: var(--cc-card);
  border: 1px solid var(--cc-line);
  color: var(--x-chipfg);
  border-radius: 4px 16px 16px 16px;
  padding: 9px 13px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.55;
}
.ctail {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 22px 12px;
}
.tray {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 10px 14px 0;
  max-height: 156px;
  overflow: auto;
}
.dchip {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: 260px;
  min-width: 0;
  padding: 5px 24px 5px 6px;
  border-radius: 10px;
  background: var(--x-chip);
  color: var(--x-chipfg);
  border: 1px solid var(--cc-line);
  font-size: 12.5px;
}
.dchip.text {
  background: var(--x-ob);
  border-color: var(--x-obl);
  color: var(--x-obfg);
}
.dchip .no {
  flex: none;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  display: grid;
  place-items: center;
  font-size: 10px;
  font-weight: 700;
  background: var(--x-acc);
  color: var(--x-acfg);
}
.dthumb {
  width: 40px;
  height: 40px;
  border-radius: 7px;
  object-fit: cover;
  flex: none;
}
.dicon {
  width: 40px;
  height: 40px;
  border-radius: 7px;
  display: grid;
  place-items: center;
  background: var(--cc-hover);
  flex: none;
}
.dicon svg {
  width: 20px;
  height: 20px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.dtext {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dx {
  all: unset;
  cursor: pointer;
  position: absolute;
  top: 3px;
  right: 3px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  display: grid;
  place-items: center;
  font-size: 14px;
  line-height: 1;
  color: inherit;
  opacity: 0.6;
}
.dx:hover {
  opacity: 1;
  background: rgba(0, 0, 0, 0.18);
}
.dropmask {
  position: absolute;
  inset: 8px;
  z-index: 5;
  border: 2px dashed var(--x-acc);
  border-radius: 16px;
  background: color-mix(in srgb, var(--x-acc) 12%, var(--cc-card));
  color: var(--x-acc);
  font-weight: 700;
  font-size: 15px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  pointer-events: none;
}
.dropmask svg {
  width: 22px;
  height: 22px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.brow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.brow .kind {
  min-width: 0;
}
.brow .acts {
  margin-left: auto;
}
</style>
