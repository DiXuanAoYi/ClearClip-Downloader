<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import type { PhoneInfo } from '../types'
import { formatDateTime } from '../utils/format'

const info = ref<PhoneInfo | null>(null)
const busy = ref(false)
const showToken = ref(false)
const DOC_URL = 'https://github.com/DiXuanAoYi/ClearClip-Downloader/blob/main/docs/phone-send.md'
const EXT_URL = 'https://github.com/DiXuanAoYi/ClearClip-Downloader/tree/main/desktop/extension'
let unlisten: UnlistenFn | undefined
let timer: number | undefined

async function refresh() {
  try {
    info.value = await api.phoneInfo()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function toggle(on: boolean) {
  busy.value = true
  try {
    info.value = await api.phoneEnable(on)
    if (on && info.value.running) ElMessage.success('已开启。第一次开启时系统可能弹出防火墙提示，请允许“专用网络”访问。')
  } catch (e) {
    ElMessage.error(errorText(e))
    await refresh()
  } finally {
    busy.value = false
  }
}

async function resetToken() {
  try {
    await ElMessageBox.confirm('重新生成访问令牌后，旧的二维码和快捷指令都会失效，已配对的设备需要重新配对。', '重新生成令牌', { type: 'warning', confirmButtonText: '重新生成', cancelButtonText: '取消' })
  } catch {
    return
  }
  info.value = await api.phoneResetToken()
}

async function revoke(id: string) {
  info.value = await api.phoneRevoke(id)
}

async function copy(text: string) {
  await api.copyText(text)
  ElMessage.success('已复制')
}

onMounted(async () => {
  await refresh()
  unlisten = await events.onPairRequest(() => refresh())
  // 配对状态可能在别处改变，定期刷新
  timer = window.setInterval(refresh, 5000)
})
onUnmounted(() => {
  unlisten?.()
  window.clearInterval(timer)
})
</script>

<template>
  <div v-if="info" class="panel">
    <section class="card block">
      <div class="kv">
        <span>
          <b>手机发链接到电脑</b>
          <small class="mute desc">手机和电脑连同一个 Wi-Fi，扫码打开网页，粘贴链接即可让电脑开始下载。不经过任何外部服务器。</small>
        </span>
        <el-switch :model-value="info.enabled" :loading="busy" @change="toggle(!info.enabled)" />
      </div>
      <el-alert v-if="info.error" type="warning" :closable="false" show-icon :title="info.error" />
      <div v-if="info.running && info.url" class="qr-wrap">
        <div class="qr" v-html="info.qrSvg" />
        <div class="qr-info">
          <p>用手机相机或微信扫一扫，打开后添加到主屏幕更方便。</p>
          <p class="mono small selectable">{{ info.url.replace(info.token, showToken ? info.token : '••••••') }}</p>
          <div class="row">
            <el-button size="small" @click="copy(info.url!)">复制网页地址</el-button>
            <el-button size="small" link @click="showToken = !showToken">{{ showToken ? '隐藏令牌' : '显示令牌' }}</el-button>
          </div>
          <p class="mute small">第一次从新设备发送时，电脑上会弹窗请你确认配对。</p>
        </div>
      </div>
    </section>

    <section v-if="info.running && info.apiUrl" class="card block">
      <h3>分享菜单（不用打开网页）</h3>
      <p class="mute small">
        iPhone 可以创建“快捷指令”，Android 可以用 HTTP Shortcuts 等工具，加入系统分享菜单后，在抖音等 App 里点“分享 → 发送到清影”即可。
        请求方式：POST，地址和请求头如下，正文为 JSON <span class="mono">{"text": "分享内容", "device": "iphone-shortcut", "name": "我的 iPhone"}</span>。
      </p>
      <div class="kvline"><span class="mute">地址</span><span class="mono selectable">{{ info.apiUrl }}</span><el-button size="small" link @click="copy(info.apiUrl!)">复制</el-button></div>
      <div class="kvline"><span class="mute">请求头 X-Token</span><span class="mono selectable">{{ showToken ? info.token : '••••••' }}</span><el-button size="small" link @click="copy(info.token)">复制</el-button></div>
      <div class="row">
        <el-button size="small" @click="api.openUrl(DOC_URL)">查看详细设置步骤</el-button>
        <el-button size="small" @click="resetToken">重新生成令牌</el-button>
      </div>
    </section>

    <section v-if="info.running && info.url" class="card block">
      <h3>网页控制台与命令行</h3>
      <p class="mute small">
        在同一局域网的手机或其他电脑上，用浏览器打开控制台，可以查看下载进度、暂停 / 继续 / 取消任务、添加链接、搜索媒体库。
        也可以用命令行控制（先在终端里进入清影的安装目录，或把它加入 PATH）：
        <span class="mono">clearclip add 链接</span>、<span class="mono">clearclip list</span>、<span class="mono">clearclip pause</span>、<span class="mono">clearclip help</span>。
        开发者可以直接调用 <span class="mono">/api/v1/…</span> 接口（需要令牌）。
      </p>
      <div class="kvline"><span class="mute">控制台地址</span><span class="mono selectable">{{ info.url.replace('/?t=', '/console?t=').replace(info.token, showToken ? info.token : '••••••') }}</span><el-button size="small" link @click="copy(info.url!.replace('/?t=', '/console?t='))">复制</el-button></div>
      <p class="mute small">控制台和接口都用同一个令牌保护，只接受局域网内的访问，网络上是明文传输，请不要在不信任的 Wi-Fi 下开启。接口不会返回你电脑上的文件路径，也不能删除文件。</p>
    </section>

    <section class="card block">
      <h3>浏览器扩展</h3>
      <p class="mute small">
        安装“发送到清影”扩展（Chrome、Edge、Firefox），可以在网页上右键发送链接，或把当前网站的登录 Cookie 一键同步给清影。扩展只连接本机。
        <template v-if="info.running">在扩展设置里填写端口 <b class="mono">{{ info.port }}</b> 和上面的访问令牌。</template>
        <template v-else>开启上面的服务后才能使用。</template>
      </p>
      <div class="row"><el-button size="small" @click="api.openUrl(EXT_URL)">安装说明</el-button></div>
    </section>

    <section class="card block">
      <h3>已配对的设备（{{ info.devices.length }}）</h3>
      <div v-if="!info.devices.length" class="mute small">还没有配对的设备。</div>
      <div v-for="d in info.devices" :key="d.id" class="device">
        <span>{{ d.name }}</span>
        <small class="mute">配对于 {{ formatDateTime(d.addedAt) }} · 最近使用 {{ formatDateTime(d.lastSeen) }}</small>
        <el-button size="small" link @click="revoke(d.id)">撤销</el-button>
      </div>
      <p class="mute small">网页只能发送链接、查看自己发送的任务状态，不能浏览或下载电脑上的文件。只接受局域网内的访问；手机和电脑不在同一网络时无法使用。</p>
    </section>
  </div>
</template>

<style scoped>
.panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.block {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
h3 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-mute);
  font-weight: 500;
  letter-spacing: 0.08em;
}
.kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.desc {
  display: block;
  font-size: 11.5px;
  margin-top: 2px;
}
.qr-wrap {
  display: flex;
  gap: 18px;
  align-items: center;
}
.qr {
  width: 200px;
  height: 200px;
  background: #fff;
  border-radius: 8px;
  padding: 4px;
  flex-shrink: 0;
}
.qr :deep(svg) {
  width: 100%;
  height: 100%;
}
.qr-info p {
  margin: 0 0 6px;
}
.small {
  font-size: 12px;
  margin: 0;
  line-height: 1.6;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.kvline {
  display: grid;
  grid-template-columns: 110px 1fr auto;
  gap: 8px;
  align-items: center;
  font-size: 12px;
  word-break: break-all;
}
.device {
  display: grid;
  grid-template-columns: 140px 1fr auto;
  gap: 8px;
  align-items: center;
  font-size: 12.5px;
  border-top: 1px dashed var(--cc-line);
  padding-top: 6px;
}
</style>
