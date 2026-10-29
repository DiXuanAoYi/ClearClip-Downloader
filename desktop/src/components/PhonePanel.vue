<script setup lang="ts">
import { onMounted, onUnmounted, reactive, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
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

const opts = reactive({ host: '', receiveDir: '', maxUploadMb: 4096, autoExtract: false, outboxHours: 24 })
let editing = false

function syncOpts(i: PhoneInfo) {
  if (editing) return
  opts.host = i.host
  opts.receiveDir = i.receiveDir
  opts.maxUploadMb = i.maxUploadMb
  opts.autoExtract = i.autoExtract
  opts.outboxHours = i.outboxHours
}

async function refresh() {
  try {
    info.value = await api.phoneInfo()
    syncOpts(info.value)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function saveOpts() {
  editing = true
  try {
    info.value = await api.phoneSetOptions({ ...opts, host: opts.host.trim(), receiveDir: opts.receiveDir.trim() })
    ElMessage.success('已保存')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    editing = false
    if (info.value) syncOpts(info.value)
  }
}

async function pickDir() {
  const r = await open({ directory: true, multiple: false })
  if (typeof r !== 'string') return
  opts.receiveDir = r
  await saveOpts()
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
          <b>手机互传</b>
          <small class="mute desc">手机和电脑连同一个 Wi-Fi，扫码打开网页：发链接让电脑开始下载，或把文字、图片、视频、压缩包等任意文件互相传送。不经过任何外部服务器。</small>
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

    <section class="card block">
      <h3>地址与传输选项</h3>
      <div class="kvline2">
        <span class="mute">电脑地址</span>
        <el-select v-model="opts.host" size="small" filterable allow-create default-first-option clearable placeholder="自动选择" @change="saveOpts">
          <el-option v-for="a in info.addresses" :key="a.ip" :value="a.ip" :label="`${a.ip}（${a.name}${a.likely ? '' : '，可能是虚拟网卡'}）`" />
        </el-select>
      </div>
      <p class="mute small">
        二维码里用的地址：<b class="mono">{{ info.ip ?? '未找到' }}</b>{{ opts.host ? '（手动指定）' : '（自动选择）' }}。
        开了代理软件的虚拟网卡 / 增强模式 / VPN 时，自动选择会避开它们；如果手机仍然打不开，在上面选择电脑的 Wi-Fi 或有线地址（一般是 192.168.x.x 或 10.x.x.x）。
      </p>
      <el-alert v-if="info.suspicious && !opts.host" type="warning" :closable="false" show-icon title="没有找到常见的 Wi-Fi / 有线网卡" description="当前使用的地址可能手机连不上。请确认电脑已连接网络，或在上面手动选择 / 填写地址。" />
      <div class="kvline2">
        <span class="mute">接收文件夹</span>
        <span class="dirrow">
          <el-input v-model="opts.receiveDir" size="small" :placeholder="info.receiveDir" @change="saveOpts" />
          <el-button size="small" @click="pickDir">选择…</el-button>
        </span>
      </div>
      <div class="kvline2">
        <span class="mute">单个文件上限</span>
        <span class="dirrow">
          <el-input-number v-model="opts.maxUploadMb" size="small" :min="0" :step="512" :controls="false" @change="saveOpts" />
          <span class="mute small">MB，0 为不限制</span>
        </span>
      </div>
      <div class="kvline2">
        <span class="mute">发给手机的保留</span>
        <span class="dirrow">
          <el-input-number v-model="opts.outboxHours" size="small" :min="0" :max="8760" :controls="false" @change="saveOpts" />
          <span class="mute small">小时后自动撤回，0 为不过期</span>
        </span>
      </div>
      <div class="kvline2">
        <span class="mute">自动解压 zip</span>
        <span class="dirrow">
          <el-switch v-model="opts.autoExtract" size="small" @change="saveOpts" />
          <span class="mute small">收到 zip 后解压到同名文件夹（不会运行其中的任何文件）</span>
        </span>
      </div>
      <el-collapse>
        <el-collapse-item title="手机扫码打不开 / 识别不出来？" name="help">
          <div class="help small">
            <p>最常见的原因是电脑上的代理软件（Clash、v2rayN、Surge 等）开了<b>虚拟网卡 / 增强模式 / TUN</b>，系统的默认出口变成了虚拟网卡，二维码里就是虚拟地址（如 198.18.x.x），手机连不上。</p>
            <p>处理方法，按顺序试：</p>
            <ol>
              <li>在上面“电脑地址”里选择电脑的 Wi-Fi / 有线地址，重新扫码。</li>
              <li>在代理软件里让局域网直连：把 <span class="mono">192.168.0.0/16</span>、<span class="mono">10.0.0.0/8</span>、<span class="mono">172.16.0.0/12</span> 加入直连 / 绕过列表（Clash 的 “bypass” 或 “skip-proxy”，v2rayN 的路由直连规则）。</li>
              <li>系统防火墙弹窗时选“专用网络”；Windows 里如果当时点了拒绝，到“防火墙 → 允许应用通过防火墙”里给清影勾选“专用”。</li>
              <li>手机上关闭它自己的代理 / VPN，并确认和电脑在同一个 Wi-Fi（有些路由器的“访客网络 / AP 隔离”会让设备互相访问不了）。</li>
              <li>仍然不行：手机浏览器直接输入上面的地址和端口（例如 http://192.168.1.23:{{ info.port }}），再把电脑上复制的网页地址整体粘贴进去。</li>
            </ol>
          </div>
        </el-collapse-item>
      </el-collapse>
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
        安装“发送到清影”扩展（Chrome、Edge、Firefox），可以在网页上右键发送链接、接管浏览器的下载（在扩展里开启）、发现并批量发送网页里的视频，或把当前网站的登录 Cookie 一键同步给清影。扩展只连接本机。
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
      <p class="mute small">网页只能发送链接、文字和文件，下载电脑上明确发给手机的项目，不能浏览电脑上的其他文件。只接受局域网内的访问；手机和电脑不在同一网络时无法使用。</p>
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
.kvline2 {
  display: grid;
  grid-template-columns: 110px 1fr;
  gap: 8px;
  align-items: center;
  font-size: 12px;
}
.dirrow {
  display: flex;
  gap: 8px;
  align-items: center;
}
.dirrow :deep(.el-input) {
  flex: 1;
}
.help p {
  margin: 4px 0;
}
.help ol {
  margin: 4px 0;
  padding-left: 20px;
  line-height: 1.7;
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
