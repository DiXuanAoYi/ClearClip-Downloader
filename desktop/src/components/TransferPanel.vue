<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import type { PhoneInfo, TransferItem, TransferList } from '../types'
import { formatBytes, formatDateTime } from '../utils/format'
import { registerDrop } from '../utils/dropHub'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const list = ref<TransferList>({ incoming: [], outgoing: [], receiveDir: '' })
const info = ref<PhoneInfo | null>(null)
const text = ref('')
const over = ref(false)
const sending = ref(false)
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
const COLORS: Record<string, string> = { image: '#c98a3b', video: '#4b7bb5', audio: '#7a8f6a', archive: '#8a6d3b', file: '#d9772f', text: '#7a62b5' }
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
  } catch (e) {
    ElMessage.error(errorText(e))
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

async function sendPaths(paths: string[]) {
  if (!paths.length) return
  sending.value = true
  try {
    const r = await api.transferSendFiles(paths)
    if (r.added) ElMessage.success(`已加入 ${r.added} 个文件，在手机网页的“电脑发来的”里下载`)
    for (const s of r.skipped.slice(0, 3)) ElMessage.warning(s)
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    sending.value = false
  }
}

async function pick() {
  const r = await open({ multiple: true })
  if (!r) return
  await sendPaths(Array.isArray(r) ? r : [r])
}

async function sendText() {
  const t = text.value.trim()
  if (!t) return
  try {
    await api.transferSendText(t)
    text.value = ''
    ElMessage.success('已加入，手机网页里可以一键复制')
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
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

onMounted(async () => {
  await Promise.all([load(), loadInfo()])
  unlisten = [
    await events.onTransferChanged(scheduleLoad),
    await events.onTransferProgress((p) => onProgress(p.id, p.done)),
    await events.onPairRequest(loadInfo),
  ]
  // 整个窗口的文件拖放事件只有一个入口：这个页面可见时，拖进来的文件都是要发给手机的
  unregister = registerDrop({
    accepts: () => true,
    over: () => (over.value = true),
    leave: () => (over.value = false),
    drop: (paths) => {
      over.value = false
      sendPaths(paths)
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
          <div class="dhead"><b>已配对的设备</b><span class="mute small">{{ info?.devices.length ?? 0 }} 台</span></div>
          <div v-if="!info?.devices.length" class="mute small">还没有配对的设备。</div>
          <div v-for="d in info?.devices" :key="d.id" class="dev">
            <span class="avatar"><svg viewBox="0 0 24 24"><rect x="7" y="2" width="10" height="20" rx="2.5" /><path d="M11 18h2" /></svg></span>
            <span class="dinfo">
              <b class="ellipsis">{{ d.name }}</b>
              <small class="mute"><span v-if="online(d.lastSeen)" class="on">● 在线</span><template v-else>最近 {{ formatDateTime(d.lastSeen) }}</template></small>
            </span>
            <a class="link" @click="revoke(d.id, d.name)">撤销</a>
          </div>
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
        <div ref="feedEl" class="feed">
          <div v-if="!rows.length" class="empty mute">
            <b>还没有传输记录</b>
            <span>手机扫码后，发来的文件、图片、链接和文字会出现在这里；<br>把文件拖进窗口，或点下面的“选择文件”，就能发给手机。</span>
          </div>
          <template v-for="r in rows" :key="r.it.id">
            <div v-if="r.day" class="day">{{ r.day }}</div>
            <div class="msg" :class="r.it.direction">
              <div class="who">{{ r.it.direction === 'in' ? r.it.deviceName || '手机' : `发给${info?.devices[0]?.name ?? '手机'}` }}</div>
              <div class="bub">
                <div v-if="r.it.kind === 'text'" class="txt selectable">{{ r.it.text }}</div>
                <div class="line">
                  <span class="thumb" :style="{ background: colorOf(r.it.kind) }"><svg viewBox="0 0 24 24" v-html="iconOf(r.it.kind)" /></span>
                  <span class="meta">
                    <b class="ellipsis" :title="r.it.kind === 'text' ? r.it.text : r.it.path">{{ r.it.kind === 'text' ? '文字' : r.it.name }}</b>
                    <small class="mute">
                      <template v-if="r.it.kind !== 'text'">{{ formatBytes(r.it.size) }} · </template>
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
          </template>
        </div>

        <div class="composer">
          <div class="drop" :class="{ on: over }">
            <svg viewBox="0 0 24 24"><path d="M12 16V4M7 9l5-5 5 5" /><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3" /></svg>
            把文件拖到这里，发送到手机（可多个；文件夹请先压缩）
          </div>
          <div class="send-row">
            <el-input v-model="text" class="grow" size="large" placeholder="输入一段文字，手机网页里一键复制…" maxlength="20000" @keydown.enter.exact.prevent="sendText" />
            <el-button size="large" :loading="sending" @click="pick">选择文件</el-button>
            <el-button size="large" type="primary" :disabled="!text.trim()" @click="sendText">发送</el-button>
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
  --acc: #d9772f;
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  gap: 0;
  border: 1px solid var(--cc-line);
  border-radius: 14px;
  overflow: hidden;
  background: var(--el-bg-color);
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
  background: color-mix(in srgb, #2f9d5b 14%, transparent);
  color: #2f9d5b;
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
  background: var(--el-fill-color-blank);
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
  border: 1px solid #efe9e0;
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
  color: var(--el-color-warning);
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
  color: #2f9d5b;
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
  color: var(--acc);
  font-size: 12px;
  font-weight: 600;
  word-break: break-all;
}
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--el-fill-color-light);
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
  background: color-mix(in srgb, var(--cc-mute) 16%, transparent);
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
  background: var(--el-bg-color);
  border: 1px solid var(--cc-line);
  border-top-left-radius: 4px;
}
.msg.out .bub {
  background: #fbe9d9;
  border-color: #f3d3b6;
  border-top-left-radius: 16px;
  border-top-right-radius: 4px;
  color: #2a2622;
}
.msg.out .mute {
  color: #8a8279;
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
  color: #2f9d5b;
}
.bad {
  color: var(--el-color-danger);
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.composer {
  padding: 12px 28px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.drop {
  border: 2px dashed #dcc9b4;
  background: color-mix(in srgb, var(--acc) 6%, transparent);
  border-radius: 16px;
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: #a0662f;
  font-size: 13px;
  font-weight: 600;
}
.drop svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.drop.on,
.over .drop {
  border-color: var(--acc);
  background: color-mix(in srgb, var(--acc) 14%, transparent);
}
.send-row {
  display: flex;
  gap: 10px;
  align-items: center;
}
.send-row .grow {
  flex: 1;
}
.send-row :deep(.el-input__wrapper) {
  border-radius: 14px;
}
.send-row :deep(.el-button) {
  border-radius: 14px;
  margin-left: 0;
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
</style>
