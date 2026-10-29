<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import type { PhoneInfo, TransferItem, TransferKind, TransferList } from '../types'
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

const KIND: Record<TransferKind, string> = { text: '文字', image: '图片', video: '视频', audio: '音频', archive: '压缩包', file: '文件' }

const hasPeer = computed(() => !!info.value?.devices.length)

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

async function clear(direction: 'in' | 'out') {
  await api.transferClear(direction)
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
  <div class="transfer" :class="{ over }">
    <el-alert v-if="info && !info.enabled" type="info" :closable="false" show-icon title="手机互传还没有开启" description="开启后，手机扫码就能把文字、图片、视频、压缩包等任意文件发给电脑，也能从手机下载电脑发来的文件。只在局域网内传输，不经过任何外部服务器。">
      <template #default><el-button size="small" type="primary" @click="enable">开启</el-button></template>
    </el-alert>
    <el-alert v-else-if="info && info.error" type="warning" :closable="false" show-icon :title="info.error" />

    <div v-if="info?.running && info.url" class="conn card">
      <div class="qr" v-html="info.qrSvg" />
      <div class="conn-info">
        <b>手机扫码打开</b>
        <small class="mute">手机和电脑连同一个 Wi-Fi。{{ hasPeer ? '' : '第一次扫码时，电脑上会弹窗请你确认配对。' }}</small>
        <small class="mono selectable">{{ info.ip }}:{{ info.port }}</small>
        <small v-if="info.suspicious" class="warn">没有找到常见的 Wi-Fi / 有线网卡，当前地址可能是代理软件的虚拟网卡。手机扫不出来或打不开时，请到“设置 → 手机与浏览器扩展”里选择电脑的局域网地址。</small>
        <div class="row">
          <el-button size="small" @click="copy(info.url!)">复制网页地址</el-button>
          <el-button size="small" link @click="app.goSettings('phone')">地址和选项</el-button>
        </div>
      </div>
    </div>

    <div class="cols">
      <section class="col card">
        <div class="col-head">
          <b>发送到手机</b>
          <span class="spacer" />
          <el-button size="small" type="primary" :loading="sending" @click="pick">选择文件…</el-button>
          <el-button v-if="list.outgoing.some((i) => !i.active)" size="small" link @click="clear('out')">清空</el-button>
        </div>
        <div class="drop" :class="{ on: over }">把文件拖到窗口里（可多个）。文件夹请先压缩成压缩包。</div>
        <div class="compose">
          <el-input v-model="text" type="textarea" :rows="2" resize="none" placeholder="也可以发一段文字，手机网页里一键复制" maxlength="20000" @keydown.ctrl.enter="sendText" @keydown.meta.enter="sendText" />
          <el-button size="small" :disabled="!text.trim()" @click="sendText">发送文字</el-button>
        </div>
        <div v-if="!list.outgoing.length" class="empty mute">还没有发给手机的内容。</div>
        <div v-for="it in list.outgoing" :key="it.id" class="item">
          <div class="item-top">
            <el-tag size="small" effect="plain">{{ KIND[it.kind] }}</el-tag>
            <span class="name ellipsis" :title="it.kind === 'text' ? it.text : it.path">{{ it.name }}</span>
            <el-button size="small" link @click="remove(it)">撤回</el-button>
          </div>
          <el-progress v-if="it.active" :percentage="pct(it)" :stroke-width="6" />
          <small class="mute">
            <template v-if="it.kind !== 'text'">{{ formatBytes(it.size) }} · </template>
            <template v-if="it.missing"><span class="bad">原文件已被移动或删除</span> · </template>
            <template v-else-if="it.active">手机正在下载 · </template>
            <template v-else>已下载 {{ it.downloads }} 次 · </template>
            {{ expiry(it) }}
          </small>
        </div>
      </section>

      <section class="col card">
        <div class="col-head">
          <b>收到的文件</b>
          <span class="spacer" />
          <el-button v-if="list.receiveDir" size="small" @click="reveal(list.receiveDir)">打开接收文件夹</el-button>
          <el-button v-if="list.incoming.some((i) => i.status !== 'receiving')" size="small" link @click="clear('in')">清空记录</el-button>
        </div>
        <div v-if="!list.incoming.length" class="empty mute">手机发来的文件、图片、视频、压缩包和文字会出现在这里。</div>
        <div v-for="it in list.incoming" :key="it.id" class="item">
          <div class="item-top">
            <el-tag size="small" effect="plain">{{ KIND[it.kind] }}</el-tag>
            <span class="name ellipsis" :title="it.path || it.text">{{ it.name }}</span>
            <template v-if="it.status === 'done' && it.kind === 'text'">
              <el-button size="small" link @click="copy(it.text)">复制</el-button>
            </template>
            <template v-else-if="it.status === 'done' && !it.missing">
              <el-button size="small" link @click="openIt(it.path)">打开</el-button>
              <el-button size="small" link @click="reveal(it.path)">显示位置</el-button>
            </template>
            <el-button size="small" link @click="remove(it)">{{ it.status === 'receiving' ? '取消' : '删除' }}</el-button>
          </div>
          <el-progress v-if="it.status === 'receiving'" :percentage="pct(it)" :stroke-width="6" />
          <pre v-if="it.kind === 'text'" class="note selectable">{{ it.text }}</pre>
          <small class="mute">
            <template v-if="it.kind !== 'text'">{{ formatBytes(it.size) }} · </template>
            {{ it.deviceName || '手机' }} · {{ formatDateTime(it.createdAt) }}
            <template v-if="it.status === 'receiving'"> · 接收中 {{ formatBytes(it.done) }}</template>
            <template v-else-if="it.status === 'failed'"> · <span class="bad">失败：{{ it.message }}</span></template>
            <template v-else-if="it.missing"> · <span class="bad">文件已被移动或删除</span></template>
            <template v-else-if="it.message"> · {{ it.message }}</template>
          </small>
          <small v-if="it.extra" class="mute">解压到 <a class="link" @click="reveal(it.extra!)">{{ it.extra }}</a></small>
        </div>
        <small class="mute foot">保存在 {{ list.receiveDir }}。收到的文件不会被自动运行，打开前请确认来源可信。</small>
      </section>
    </div>
  </div>
</template>

<style scoped>
.transfer {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.conn {
  display: flex;
  gap: 16px;
  align-items: center;
  padding: 12px 14px;
}
.qr {
  width: 132px;
  height: 132px;
  flex-shrink: 0;
  background: #fff;
  border-radius: 8px;
  padding: 3px;
}
.qr :deep(svg) {
  width: 100%;
  height: 100%;
}
.conn-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.warn {
  color: var(--el-color-warning);
  font-size: 12px;
  line-height: 1.5;
}
.cols {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  align-items: start;
}
@media (max-width: 900px) {
  .cols {
    grid-template-columns: 1fr;
  }
}
.col {
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.col-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.col-head :deep(.el-button) {
  margin-left: 0;
}
.spacer {
  flex: 1;
}
.drop {
  border: 1px dashed var(--cc-line);
  border-radius: 8px;
  padding: 12px;
  text-align: center;
  font-size: 12px;
  color: var(--cc-mute);
}
.drop.on,
.over .drop {
  border-color: var(--el-color-primary);
  color: var(--el-color-primary);
  background: color-mix(in srgb, var(--el-color-primary) 8%, transparent);
}
.compose {
  display: flex;
  gap: 8px;
  align-items: flex-end;
}
.compose :deep(.el-textarea) {
  flex: 1;
}
.empty {
  padding: 18px 0;
  text-align: center;
  font-size: 12.5px;
}
.item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border-top: 1px dashed var(--cc-line);
  padding-top: 8px;
}
.item-top {
  display: flex;
  align-items: center;
  gap: 8px;
}
.item-top :deep(.el-button) {
  margin-left: 0;
}
.name {
  flex: 1;
  min-width: 0;
  font-size: 13px;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.note {
  margin: 0;
  padding: 6px 8px;
  background: var(--cc-bg, rgba(127, 127, 127, 0.08));
  border-radius: 6px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 9em;
  overflow: auto;
  font-size: 12px;
}
.bad {
  color: var(--el-color-danger);
}
.link {
  cursor: pointer;
  color: var(--el-color-primary);
  word-break: break-all;
}
.foot {
  margin-top: 4px;
  line-height: 1.5;
}
</style>
