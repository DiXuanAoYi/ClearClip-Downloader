<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { Settings, UpdateInfo } from '../types'
import { previewFilename } from '../utils/format'
import AccountsPanel from '../components/AccountsPanel.vue'
import NetworkPanel from '../components/NetworkPanel.vue'
import ComponentsPanel from '../components/ComponentsPanel.vue'
import PhonePanel from '../components/PhonePanel.vue'
import AiPanel from '../components/AiPanel.vue'
import SiteRulesPanel from '../components/SiteRulesPanel.vue'
import NotifyPanel from '../components/NotifyPanel.vue'
import UploadPanel from '../components/UploadPanel.vue'
import RulesPanel from '../components/RulesPanel.vue'
import SecurityPanel from '../components/SecurityPanel.vue'
import ProtocolsPanel from '../components/ProtocolsPanel.vue'
import type { HealthResult } from '../types'
import { copyDiagnostics } from '../composables/diagnostics'
import { LANGUAGES } from '../i18n'

const app = useAppStore()
const form = ref<Settings>(JSON.parse(JSON.stringify(app.settings!)))
const saving = ref(false)
const update = ref<UpdateInfo | null>(null)
const checking = ref(false)
// 快捷键在输入框失焦后才生效，避免输入到一半就注册
const shortcutDraft = ref(form.value.shortcut)
const domainsText = ref(form.value.clipboardDomains.join('\n'))
const subLangsText = ref(form.value.subtitleLangs.join(', '))

function saveSubLangs() {
  form.value.subtitleLangs = subLangsText.value
    .split(/[,，\s]+/)
    .map((l) => l.trim())
    .filter(Boolean)
  subLangsText.value = form.value.subtitleLangs.join(', ')
}
const DAYS = ['一', '二', '三', '四', '五', '六', '日']

function addSpeedWindow() {
  form.value.speedSchedule.push({ days: [], start: '08:00', end: '23:00', limitKbps: 1024 })
}
const health = ref<HealthResult[] | null>(null)
const checkingHealth = ref(false)
const portable = ref(false)
api.isPortable().then((v) => (portable.value = v)).catch(() => {})

function saveDomains() {
  form.value.clipboardDomains = domainsText.value
    .split(/[\s,，]+/)
    .map((d) => d.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, '').replace(/^\*?\./, ''))
    .filter((d) => d.includes('.'))
  domainsText.value = form.value.clipboardDomains.join('\n')
}

async function runHealth() {
  checkingHealth.value = true
  health.value = null
  try {
    health.value = await api.healthCheck()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    checkingHealth.value = false
  }
}

const nameSample = { author: '山野厨房', title: '秋天第一锅板栗焖鸡 #家常菜', id: '7421234567890123456', platform: '抖音', quality: '1080P' }
const namePreview = computed(() => previewFilename(form.value.filenameTemplate, nameSample))
/** 模板里不认识的 {变量}：会原样写进文件名，提前提醒。 */
const NAME_VARS = ['author', 'title', 'date', 'id', 'platform', 'quality']
const unknownVars = computed(() => [...new Set([...form.value.filenameTemplate.matchAll(/\{([^{}]*)\}/g)].map((m) => m[1]).filter((v) => !NAME_VARS.includes(v)))])
const unknownText = computed(() => unknownVars.value.map((v) => `{${v}}`).join('、'))

// 任一项修改后自动保存
let timer: number | undefined
watch(
  form,
  () => {
    window.clearTimeout(timer)
    timer = window.setTimeout(save, 400)
  },
  { deep: true },
)

async function save() {
  saving.value = true
  try {
    await app.save(form.value)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    saving.value = false
  }
}

async function pickDir() {
  const dir = await open({ directory: true, defaultPath: form.value.downloadDir || undefined, title: '选择保存位置' })
  if (typeof dir === 'string') form.value.downloadDir = dir
}

async function pickTempDir() {
  const dir = await open({ directory: true, defaultPath: form.value.tempDir || undefined, title: '选择临时文件目录' })
  if (typeof dir === 'string') form.value.tempDir = dir
}

const installing = ref(false)
async function installUpdate() {
  installing.value = true
  try {
    await api.installUpdate()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    installing.value = false
  }
}

async function checkUpdate() {
  checking.value = true
  try {
    update.value = await api.checkUpdate()
    if (!update.value.hasUpdate) ElMessage.success(update.value.latest ? '已是最新版本' : '还没有发布过正式版本')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    checking.value = false
  }
}

async function call(fn: () => Promise<unknown>) {
  try {
    await fn()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function insertVar(v: string) {
  form.value.filenameTemplate += v
}
</script>

<template>
  <div class="page">
    <div class="head">
      <h2>设置</h2>
      <span class="mute saving">{{ saving ? '保存中…' : '修改后自动保存' }}</span>
    </div>

    <el-tabs v-model="app.settingsTab">
      <el-tab-pane label="下载" name="download">
        <div class="groups">
          <section class="group card">
            <h3>保存</h3>
            <div class="field">
              <label>保存位置</label>
              <div class="inline">
                <el-input v-model="form.downloadDir" size="small" class="grow" />
                <el-button size="small" @click="pickDir">选择…</el-button>
              </div>
            </div>
            <div class="kv"><span>按平台分子文件夹</span><el-switch v-model="form.subfolderByPlatform" /></div>
            <div class="field">
              <label>文件命名</label>
              <el-input v-model="form.filenameTemplate" size="small" class="mono" />
              <div class="vars">
                <button v-for="v in ['{author}', '{title}', '{date}', '{id}', '{platform}', '{quality}']" :key="v" type="button" class="var mono" @click="insertVar(v)">{{ v }}</button>
              </div>
              <small class="mute ellipsis">示例：{{ namePreview }}</small>
              <small v-if="unknownVars.length" class="warn">不认识的变量 {{ unknownText }}，会原样写进文件名。</small>
              <small class="mute block">手动下载、识别后自动下载、手机发送、订阅都按这个命名，下载队列和完成通知里显示的也是这个名字。</small>
            </div>
            <div class="kv"><span>跳过已下载过的内容</span><el-switch v-model="form.skipExisting" /></div>
            <div class="kv">
              <span>目标文件已存在时</span>
              <el-select v-model="form.conflictPolicy" size="small" class="sel">
                <el-option value="rename" label="自动重命名" />
                <el-option value="skip" label="跳过" />
                <el-option value="overwrite" label="覆盖" />
              </el-select>
            </div>
            <div class="field">
              <label>临时文件目录</label>
              <div class="inline">
                <el-input v-model="form.tempDir" size="small" class="grow" placeholder="留空表示与保存位置相同" />
                <el-button size="small" @click="pickTempDir">选择…</el-button>
              </div>
              <small class="mute">可指定到空间更大的磁盘。合并音视频时需要约两倍的临时空间。</small>
            </div>
            <div class="kv">
              <span>保留的磁盘剩余空间（MB）<small class="mute block">空间不足时暂停下载并提示</small></span>
              <el-input-number v-model="form.diskReserveMb" :min="0" :max="102400" :step="100" size="small" />
            </div>
            <div class="kv"><span>全部下载完成后发送通知</span><el-switch v-model="form.notifyOnComplete" /></div>
          </section>

          <section class="group card">
            <h3>队列与断点续传</h3>
            <div class="kv"><span>同时下载数</span><el-input-number v-model="form.concurrency" :min="1" :max="8" size="small" /></div>
            <div class="kv">
              <span>每个网站同时下载数<small class="mute block">使用登录账号的网站自动减半，降低被限制的风险</small></span>
              <el-input-number v-model="form.perSiteConcurrency" :min="1" :max="8" size="small" />
            </div>
            <div class="kv">
              <span>大文件分段数<small class="mute block">服务器支持时多线程并行下载，1 表示不分段</small></span>
              <el-input-number v-model="form.segments" :min="1" :max="64" size="small" />
            </div>
            <div class="kv"><span>文件大于多少 MB 时分段</span><el-input-number v-model="form.segmentMinMb" :min="2" :max="1024" size="small" /></div>
            <div class="kv">
              <span>全局限速（KB/s）<small class="mute block">0 表示不限速</small></span>
              <el-input-number v-model="form.speedLimitKbps" :min="0" :max="1048576" :step="256" size="small" />
            </div>
            <div class="kv">
              <span>启动时自动继续未完成的任务<small class="mute block">关闭时恢复为“已暂停”，手动继续</small></span>
              <el-switch v-model="form.autoResume" />
            </div>
            <div class="kv"><span>网络中断时自动重试</span><el-input-number v-model="form.maxRetries" :min="0" :max="10" size="small" /></div>
            <div class="kv">
              <span>第一次重试前等待（秒）<small class="mute block">之后每次等待时间翻倍</small></span>
              <el-input-number v-model="form.retryDelaySecs" :min="1" :max="60" size="small" />
            </div>
            <div class="kv">
              <span>取消任务时保留已下载部分<small class="mute block">保留后重新加入同一资源可接着下载</small></span>
              <el-switch v-model="form.keepPartOnCancel" />
            </div>
            <small class="mute">关闭程序、断网或下载地址过期后，任务都会从已下载的位置继续；服务器不支持续传时会自动从头下载。</small>
          </section>

          <section class="group card">
            <h3>限速计划与省流量</h3>
            <div class="col">
              <span>限速计划<small class="mute block">在指定时段改用另一个限速，没命中任何时段时使用上面的全局限速；多个时段重叠时取最先匹配的</small></span>
              <div v-for="(w, i) in form.speedSchedule" :key="i" class="win">
                <el-checkbox-group v-model="w.days" size="small">
                  <el-checkbox-button v-for="(d, k) in DAYS" :key="k" :value="k + 1">{{ d }}</el-checkbox-button>
                </el-checkbox-group>
                <el-time-select v-model="w.start" size="small" class="tsel" start="00:00" end="23:30" step="00:30" :clearable="false" />
                <span class="mute small">到</span>
                <el-time-select v-model="w.end" size="small" class="tsel" start="00:00" end="23:30" step="00:30" :clearable="false" />
                <el-input-number v-model="w.limitKbps" :min="0" :max="1048576" :step="256" size="small" class="lim" />
                <span class="mute small">KB/s</span>
                <el-button link size="small" @click="form.speedSchedule.splice(i, 1)">删除</el-button>
              </div>
              <div>
                <el-button size="small" @click="addSpeedWindow">添加时段</el-button>
                <span class="mute small"> {{ form.speedSchedule.length ? '限速填 0 表示这个时段不限速；不选星期表示每天；结束早于开始表示跨过午夜' : '例如白天限速、半夜不限速' }}</span>
              </div>
            </div>
            <div class="kv">
              <span>省流量模式<small class="mute block">用于手机热点等按流量计费的网络：下载并发降为 1、分段数减少、默认画质改为省空间（不超过 720p）、不自动下载订阅更新、不自动上传和调用 AI，已排队的任务不受影响</small></span>
              <el-switch v-model="form.meteredMode" />
            </div>
          </section>

          <section class="group card">
            <h3>整理</h3>
            <div class="kv">
              <span>把标题、作者、封面写入文件<small class="mute block">需要 ffmpeg；视频和音频在播放器、音乐软件里能显示信息</small></span>
              <el-switch v-model="form.embedMetadata" />
            </div>
            <div class="kv"><span>保存作品信息 JSON<small class="mute block">标题、作者、原链接、发布时间等，与视频同名</small></span><el-switch v-model="form.writeInfoJson" /></div>
            <div class="kv"><span>生成 NFO 文件<small class="mute block">Jellyfin、Kodi、Plex 等媒体服务器能识别</small></span><el-switch v-model="form.writeNfo" /></div>
            <div class="kv">
              <span>音视频合并后的格式<small class="mute block">MKV 兼容的编码更多，MP4 播放兼容性更好</small></span>
              <el-select v-model="form.mergeContainer" size="small" class="sel">
                <el-option value="mp4" label="MP4" />
                <el-option value="mkv" label="MKV" />
              </el-select>
            </div>
          </section>

          <section class="group card">
            <h3>完成后</h3>
            <div class="kv">
              <span>下载后检查文件<small class="mute block">用 ffmpeg 检查视频能否正常读取；损坏时自动清理并重新下载一次。需要安装 ffmpeg</small></span>
              <el-switch v-model="form.verifyDownloads" />
            </div>
            <div class="kv"><span>全部完成后打开下载文件夹</span><el-switch v-model="form.openFolderOnDone" /></div>
            <div class="kv"><span>有下载任务时阻止系统休眠</span><el-switch v-model="form.preventSleep" /></div>
            <div class="kv">
              <span>每个任务完成后运行命令<small class="mute block">高级功能，只运行你自己填写的命令</small></span>
              <el-switch v-model="form.postScriptEnabled" />
            </div>
            <div v-if="form.postScriptEnabled" class="field">
              <el-input v-model="form.postScript" size="small" class="mono" placeholder='例如：python "D:\tools\upload.py"' />
              <small class="mute">文件路径等信息通过环境变量传入：CLEARCLIP_FILE、CLEARCLIP_TITLE、CLEARCLIP_AUTHOR、CLEARCLIP_URL、CLEARCLIP_PLATFORM。</small>
            </div>
            <small class="mute">“全部完成后睡眠 / 关机”在下载队列页面设置，只对本次运行有效。</small>
          </section>
        </div>
      </el-tab-pane>

      <el-tab-pane label="解析" name="parse">
        <div class="groups">
          <section class="group card">
            <h3>剪贴板</h3>
            <div class="kv"><span>监听剪贴板</span><el-switch v-model="form.watchClipboard" /></div>
            <div class="kv">
              <span>识别后自动下载<small class="mute block">按默认选项直接加入队列，不弹出确认；保存位置和文件命名同“下载 → 保存”</small></span>
              <el-switch v-model="form.autoDownload" :disabled="!form.watchClipboard" />
            </div>
            <div class="field">
              <label>全局快捷键（解析剪贴板）</label>
              <el-input v-model="shortcutDraft" size="small" placeholder="CommandOrControl+Shift+D" class="mono" @change="form.shortcut = shortcutDraft.trim()" />
              <small v-if="app.shortcutError" class="err">{{ app.shortcutError }}</small>
              <small v-else class="mute">格式如 CommandOrControl+Shift+D，留空表示不使用。</small>
            </div>
            <div class="kv">
              <span>识别所有网址<small class="mute block">关闭时只识别内置平台和下面列出的网站，避免复制普通网址时频繁弹窗</small></span>
              <el-switch v-model="form.clipboardAllSites" />
            </div>
            <div v-if="!form.clipboardAllSites" class="field">
              <label>额外识别的网站</label>
              <el-input v-model="domainsText" type="textarea" :rows="3" size="small" class="mono" placeholder="youtube.com" @change="saveDomains" />
              <small class="mute">每行一个域名，同时匹配子域名。</small>
            </div>
          </section>
          <section class="group card">
            <h3>收到的链接</h3>
            <small class="mute">手机、浏览器扩展发来的链接和手动解析失败的链接会记录在“媒体库 → 收到的链接”，可以随时重试。</small>
            <div class="kv">
              <span>记录剪贴板识别到的链接<small class="mute block">关闭后不再记录剪贴板里的链接</small></span>
              <el-switch v-model="form.inbox.recordClipboard" />
            </div>
            <div class="kv">
              <span>保留天数<small class="mute block">0 表示不按时间清理；进行中的不会被清理</small></span>
              <el-input-number v-model="form.inbox.keepDays" :min="0" :max="3650" size="small" />
            </div>
            <div class="kv">
              <span>最多保留条数<small class="mute block">0 表示不限</small></span>
              <el-input-number v-model="form.inbox.maxItems" :min="0" :max="100000" :step="100" size="small" />
            </div>
          </section>
          <section class="group card">
            <h3>字幕与弹幕</h3>
            <small class="mute">解析结果里会列出字幕轨：人工上传的字幕、YouTube 的自动生成字幕和偏好语言的自动翻译、B站的 CC / AI 字幕（需要登录）和弹幕。</small>
            <div class="field">
              <label>偏好语言（靠前的优先）</label>
              <el-input v-model="subLangsText" size="small" class="mono" placeholder="zh, en" @change="saveSubLangs" />
              <small class="mute">用逗号分隔，如 <span class="mono">zh, en, ja</span>。<span class="mono">zh</span> 同时匹配简体、繁体等变体。偏好语言的字幕在列表里排前面，也是“自动选中”的依据。</small>
            </div>
            <div class="kv">
              <span>下载视频时同时处理字幕<small class="mute block">对剪贴板自动下载、播放列表和订阅生效；解析页里也会默认选中偏好语言的字幕</small></span>
              <el-select v-model="form.subtitleMode" size="small" class="sel">
                <el-option value="off" label="不处理" />
                <el-option value="file" label="保存为字幕文件" />
                <el-option value="embed" label="内嵌到视频" />
                <el-option value="burn" label="烧录进画面" />
              </el-select>
            </div>
            <div class="kv">
              <span>包含自动生成和自动翻译的字幕<small class="mute block">YouTube 没有人工字幕时很有用，质量不如人工字幕</small></span>
              <el-switch v-model="form.subtitleAuto" />
            </div>
            <div class="kv">
              <span>VTT 字幕转成 SRT<small class="mute block">SRT 是播放器和剪辑软件通用的格式；自动字幕的重复滚动文字也会一并整理</small></span>
              <el-switch v-model="form.subtitleConvert" />
            </div>
            <div class="kv">
              <span>B站弹幕转成 ASS<small class="mute block">播放器（PotPlayer、VLC、mpv 等）可以直接显示滚动弹幕；关闭则保存 XML 原文</small></span>
              <el-switch v-model="form.danmakuAss" />
            </div>
            <template v-if="form.danmakuAss">
              <div class="kv">
                <span>弹幕字号<small class="mute block">以 1080 高度的画面为准</small></span>
                <el-input-number v-model="form.danmaku.fontSize" size="small" :min="18" :max="80" controls-position="right" />
              </div>
              <div class="kv">
                <span>弹幕不透明度（%）</span>
                <el-slider v-model="form.danmaku.opacity" :min="20" :max="100" :step="5" size="small" class="slim" />
              </div>
              <div class="kv">
                <span>滚动一屏的时间（秒）<small class="mute block">越小越快</small></span>
                <el-input-number v-model="form.danmaku.scrollSecs" size="small" :min="3" :max="20" controls-position="right" />
              </div>
              <div class="kv">
                <span>弹幕占画面高度（%）<small class="mute block">留出底部给字幕</small></span>
                <el-slider v-model="form.danmaku.area" :min="10" :max="100" :step="5" size="small" class="slim" />
              </div>
              <div class="field">
                <label>弹幕字体</label>
                <el-input v-model="form.danmaku.font" size="small" placeholder="留空使用微软雅黑（没有时播放器自动换用其他中文字体）" />
              </div>
            </template>
          </section>
          <section class="group card">
            <h3>解析方式</h3>
            <div class="field">
              <label>解析模式</label>
              <el-select v-model="form.parseMode" size="small">
                <el-option value="local_then_remote" label="本地解析，失败时用远程 API" />
                <el-option value="local" label="只用本地解析" />
                <el-option value="remote" label="只用远程 API" />
              </el-select>
            </div>
            <div class="kv">
              <span>同一网站两次解析的最小间隔（毫秒）<small class="mute block">批量解析时避免请求过快被风控</small></span>
              <el-input-number v-model="form.siteRequestIntervalMs" :min="0" :max="10000" :step="100" size="small" />
            </div>
            <div class="field">
              <label>远程 API 地址</label>
              <el-input v-model="form.remoteEndpoint" size="small" placeholder="https://your-host/api" class="mono" />
              <small class="mute">填写你自己部署的解析接口地址，只用于抖音、快手；其他平台始终本地解析。留空则不使用。</small>
            </div>
          </section>
          <section class="group card">
            <h3>清晰度与格式</h3>
            <div class="kv">
              <span>默认清晰度<small class="mute block">解析后默认选中，剪贴板自动下载和批量下载也按这里选择</small></span>
              <el-select v-model="form.qualityPreset" size="small" class="sel">
                <el-option value="best" label="最高画质" />
                <el-option value="max1080" label="不超过 1080P" />
                <el-option value="small" label="省空间（≤720P）" />
                <el-option value="audio" label="只要音频" />
              </el-select>
            </div>
            <div class="kv">
              <span>大小上限（MB）<small class="mute block">默认选中的格式预估超过上限时，改选不超过的最高清晰度；0 表示不限制。网站没给出大小时按码率和时长估算</small></span>
              <el-input-number v-model="form.maxSizeMb" size="small" :min="0" :max="1000000" :step="50" controls-position="right" />
            </div>
            <div class="kv">
              <span>失败时自动降级<small class="mute block">网络、文件损坏等原因下载失败时，自动改用低一档的清晰度重新下载（最多 3 次）</small></span>
              <el-switch v-model="form.autoDowngrade" />
            </div>
            <div class="kv">
              <span>同等清晰度优先 H.264<small class="mute block">兼容性最好；AV1、H.265 体积更小，但老设备可能播不了</small></span>
              <el-switch v-model="form.preferH264" />
            </div>
            <div class="kv">
              <span>只要音频时的格式</span>
              <el-select v-model="form.audioFormat" size="small" class="sel">
                <el-option value="mp3" label="MP3" />
                <el-option value="m4a" label="M4A（AAC，不转码）" />
                <el-option value="opus" label="Opus" />
                <el-option value="flac" label="FLAC" />
              </el-select>
            </div>
            <div class="field">
              <label>剧集 / 合集命名</label>
              <el-input v-model="form.seriesTemplate" size="small" class="mono" placeholder="{series}/第{episode}集" />
              <small class="mute">变量：{series} 剧名或列表名、{season} 季、{episode} 集数，以及 {title} {author} 等；“/” 表示子文件夹。留空则使用普通文件命名。</small>
            </div>
          </section>
          <SiteRulesPanel v-model="form.customSites" />
          <section class="group card">
            <h3>其他网站</h3>
            <div class="kv">
              <span>使用 yt-dlp 解析其他网站<small class="mute block">支持 YouTube、Pornhub、Twitter/X 等上千个网站；内置解析失效时也会用它重试</small></span>
              <el-switch v-model="form.useYtdlp" />
            </div>
            <div class="kv">
              <span>在网页中查找视频地址<small class="mute block">yt-dlp 也不支持时，尝试从网页里找 mp4 / m3u8 地址</small></span>
              <el-switch v-model="form.genericSniffer" />
            </div>
            <div class="kv">
              <span>m3u8 分片并发数</span>
              <el-input-number v-model="form.hlsConcurrency" :min="1" :max="32" size="small" />
            </div>
            <div class="kv">
              <span>跳过 m3u8 里的插播广告<small class="mute block">按分片来源判断，极少数情况下可能误删正片片段</small></span>
              <el-switch v-model="form.hlsSkipAds" />
            </div>
          </section>
        </div>
      </el-tab-pane>

      <el-tab-pane label="BT 与 ed2k" name="p2p">
        <ProtocolsPanel v-model="form" />
      </el-tab-pane>

      <el-tab-pane label="网络" name="network">
        <NetworkPanel v-model="form.network" />
      </el-tab-pane>

      <el-tab-pane label="账号与 Cookie" name="accounts">
        <AccountsPanel />
      </el-tab-pane>

      <el-tab-pane label="手机与浏览器扩展" name="phone">
        <PhonePanel />
      </el-tab-pane>

      <el-tab-pane label="自动化" name="automation">
        <div class="groups">
          <NotifyPanel v-model="form.notify" />
          <UploadPanel v-model="form.upload" />
          <RulesPanel v-model="form.rules" class="wide" />
        </div>
      </el-tab-pane>

      <el-tab-pane label="AI" name="ai">
        <AiPanel v-model="form.ai" />
      </el-tab-pane>

      <el-tab-pane label="安全与隐私" name="security">
        <SecurityPanel v-model="form.security" />
      </el-tab-pane>

      <el-tab-pane label="组件" name="components">
        <ComponentsPanel v-model="form.componentMirrors" v-model:edition="form.ffmpegEdition" />
      </el-tab-pane>

      <el-tab-pane label="诊断" name="diagnostics">
        <div class="groups">
          <section class="group card">
            <h3>问题反馈</h3>
            <p class="mute small">诊断信息包含版本、系统、设置概要、失败任务和最近日志。Cookie、令牌等敏感内容会自动隐去。</p>
            <div class="inline wrap">
              <el-button size="small" type="primary" @click="copyDiagnostics">复制诊断信息</el-button>
              <el-button size="small" @click="call(api.openLogDir)">打开日志目录</el-button>
            </div>
          </section>
          <section class="group card">
            <h3>解析样本</h3>
            <div class="kv">
              <span>保存解析时的原始响应<small class="mute block">用于排查网站改版导致的解析失败，平时不需要开启</small></span>
              <el-switch v-model="form.recordSamples" />
            </div>
            <p class="mute small">样本会去掉 Cookie、令牌和链接里的签名参数，但页面内容里仍可能有你的昵称等信息，分享前请检查。</p>
            <div class="inline"><el-button size="small" @click="call(api.openSamplesDir)">打开样本目录</el-button></div>
          </section>
          <section class="group card wide">
            <h3>平台健康检查</h3>
            <p class="mute small">用示例链接（最近一次成功解析的链接，或内置示例）测试各平台解析是否正常，用于判断是网站改版还是网络问题。</p>
            <div class="inline"><el-button size="small" type="primary" :loading="checkingHealth" @click="runHealth">开始检查</el-button></div>
            <div v-if="health" class="health">
              <div v-for="h in health" :key="h.platform" class="hrow">
                <span :class="h.ok ? 'okc' : h.sample ? 'err' : 'mute'">{{ h.ok ? '●' : h.sample ? '●' : '○' }}</span>
                <b>{{ h.name }}</b>
                <small class="ellipsis" :class="h.ok ? '' : h.sample ? 'err' : 'mute'" :title="h.message">{{ h.message }}</small>
                <small class="mono mute">{{ h.millis ? `${(h.millis / 1000).toFixed(1)}s` : '' }}</small>
              </div>
            </div>
          </section>
        </div>
      </el-tab-pane>

      <el-tab-pane label="通用" name="general">
        <div class="groups">
          <section class="group card">
            <h3>外观与行为</h3>
            <div class="kv">
              <span>外观</span>
              <el-radio-group v-model="form.theme" size="small">
                <el-radio-button value="system">跟随系统</el-radio-button>
                <el-radio-button value="dark">深色</el-radio-button>
                <el-radio-button value="light">浅色</el-radio-button>
              </el-radio-group>
            </div>
            <div class="kv">
              <span>界面语言<small class="mute block">English 界面由机器翻译整理，较长的提示和软件返回的错误消息仍可能是中文</small></span>
              <el-radio-group v-model="form.language" size="small">
                <el-radio-button v-for="l in LANGUAGES" :key="l.value" :value="l.value">{{ l.label }}</el-radio-button>
              </el-radio-group>
            </div>
            <div class="kv"><span>关闭窗口时最小化到托盘</span><el-switch v-model="form.closeToTray" /></div>
            <div class="kv"><span>悬浮拖拽窗<small class="mute block">桌面上的小窗口：把浏览器里的链接拖进去就开始下载，也可以点一下后按 Ctrl+V 粘贴；双击回到主窗口</small></span><el-switch v-model="form.floatBall" /></div>
            <div class="kv"><span>开机自动启动<small class="mute block">在托盘运行，用于订阅的定期检查</small></span><el-switch v-model="form.launchAtLogin" /></div>
            <div class="kv"><span>订阅与追更<small class="mute block">在“订阅”页面管理</small></span><el-switch v-model="form.subscriptionsEnabled" /></div>
            <div class="kv"><span>启动时检查更新</span><el-switch v-model="form.checkUpdate" /></div>
            <small v-if="portable" class="mute">便携模式：设置、数据库和日志保存在程序目录下的 data 文件夹。</small>
          </section>
          <section class="group card">
            <h3>媒体库</h3>
            <div class="kv">
              <span>删除文件时先放进回收站<small class="mute block">在媒体库里删除文件时移到下载目录里的隐藏文件夹，可以还原</small></span>
              <el-switch v-model="form.library.useTrash" />
            </div>
            <div v-if="form.library.useTrash" class="kv">
              <span>回收站保留天数<small class="mute block">超过天数的文件在启动时永久删除，0 表示不自动清空</small></span>
              <el-input-number v-model="form.library.trashKeepDays" size="small" :min="0" :max="3650" controls-position="right" />
            </div>
            <div class="kv">
              <span>下载完成后记录时长并截取封面<small class="mute block">用 ffmpeg 读取视频时长；没有封面的视频会截一张图</small></span>
              <el-switch v-model="form.library.probeNewFiles" />
            </div>
            <div class="field">
              <span>“整理文件夹”的默认规则</span>
              <el-input v-model="form.library.reorganizeTemplate" size="small" class="mono" placeholder="{platform}/{author}" />
              <small class="mute">变量：{platform} {author} {year} {month} {day} {date} {kind}</small>
            </div>
          </section>
          <section class="group card">
            <h3>关于</h3>
            <div class="kv">
              <span>清影 ClearClip v{{ app.info?.version }}</span>
              <el-button size="small" :loading="checking" @click="checkUpdate">检查更新</el-button>
            </div>
            <div v-if="update?.hasUpdate" class="kv">
              <span>新版本 v{{ update.latest }} 可用</span>
              <el-button v-if="update.canInstall" size="small" type="primary" :loading="installing" @click="installUpdate">立即更新并重启</el-button>
              <el-button v-else size="small" type="primary" @click="api.openUrl(update.url)">前往下载</el-button>
            </div>
            <p class="mute small">
              仅用于下载你有权保存的内容。项目地址：<a href="#" @click.prevent="api.openUrl(`https://github.com/${app.info?.repo}`)">github.com/{{ app.info?.repo }}</a>
            </p>
          </section>
        </div>
      </el-tab-pane>
    </el-tabs>
  </div>
</template>

<style scoped>
.wide {
  grid-column: 1 / -1;
}
.slim {
  width: 180px;
}
.page {
  padding: 20px 24px;
  max-width: 1040px;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 12px;
  margin-bottom: 4px;
}
h2 {
  margin: 0;
  font-size: 16px;
}
.saving {
  font-size: 11.5px;
}
.groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
  gap: 12px;
  align-items: start;
}
.group {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
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
.kv > span {
  min-width: 0;
}
.kv :deep(.el-radio-group) {
  flex-shrink: 0;
  flex-wrap: nowrap;
}
.block {
  display: block;
  font-size: 11px;
}
.col {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.win {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.tsel {
  width: 110px;
}
.lim {
  width: 130px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.field label {
  font-size: 12.5px;
}
.field small {
  font-size: 11px;
}
.err {
  color: var(--cc-err);
}
.warn {
  color: var(--cc-warn, #e6a23c);
}
.inline {
  display: flex;
  gap: 8px;
  align-items: center;
}
.inline :deep(.el-button) {
  margin-left: 0;
}
.wrap {
  flex-wrap: wrap;
}
.grow {
  flex: 1;
  min-width: 0;
}
.sel {
  width: 140px;
}
.vars {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.var {
  all: unset;
  cursor: pointer;
  font-size: 11px;
  padding: 0 7px;
  line-height: 20px;
  border: 1px solid var(--cc-line);
  border-radius: 4px;
  color: var(--cc-mute);
}
.var:hover {
  border-color: var(--cc-acc);
  color: var(--cc-acc);
}
.wide {
  grid-column: 1 / -1;
}
.health {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.hrow {
  display: grid;
  grid-template-columns: 14px 150px 1fr 48px;
  gap: 8px;
  align-items: center;
  font-size: 12.5px;
}
.okc {
  color: var(--cc-ok);
}
.small {
  font-size: 12px;
  margin: 0;
  line-height: 1.6;
}
a {
  color: var(--cc-acc);
}
</style>
