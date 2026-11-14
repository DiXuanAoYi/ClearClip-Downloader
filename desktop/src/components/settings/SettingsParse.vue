<script setup lang="ts">
import type { Settings } from '../../types'
import type { LayoutMode } from '../../utils/balance'
import BalancedGrid from '../BalancedGrid.vue'
import CcField from '../ui/CcField.vue'
import CcSelect from '../ui/CcSelect.vue'
import { ref } from 'vue'
import { useAppStore } from '../../stores/app'
import SiteRulesPanel from '../SiteRulesPanel.vue'
import HotkeyInput from '../HotkeyInput.vue'
import CcLink from '../ui/CcLink.vue'

const app = useAppStore()
const form = defineModel<Settings>({ required: true })
defineProps<{ layout: LayoutMode }>()

const SUBTITLE_OPTS = [{ value: 'off', label: '不处理' }, { value: 'file', label: '保存为字幕文件' }, { value: 'embed', label: '内嵌到视频' }, { value: 'burn', label: '烧录进画面' }]
const PARSE_MODE_OPTS = [{ value: 'local_then_remote', label: '本地解析，失败时用远程 API' }, { value: 'local', label: '只用本地解析' }, { value: 'remote', label: '只用远程 API' }]
const QUALITY_OPTS = [{ value: 'best', label: '最高画质' }, { value: 'max1080', label: '不超过 1080P' }, { value: 'small', label: '省空间（≤720P）' }, { value: 'audio', label: '只要音频' }]
const AUDIO_OPTS = [{ value: 'mp3', label: 'MP3' }, { value: 'm4a', label: 'M4A（AAC，不转码）' }, { value: 'opus', label: 'Opus' }, { value: 'flac', label: 'FLAC' }]
/** 其他全局快捷键（录入时查重）。 */
function others(self: 'shortcut' | 'shotHotkey') {
  const all = [
    { key: 'shortcut', name: '解析剪贴板', value: form.value.shortcut },
    { key: 'shotHotkey', name: '截图翻译', value: form.value.shotHotkey },
    { key: 'panic', name: '老板键', value: form.value.security.panicShortcut },
    { key: 'write', name: '写作窗', value: form.value.writing.hotkey },
  ]
  return all.filter((o) => o.key !== self)
}
const REMOTE_PH = 'https://your-host/api'
const domainsText = ref(form.value.clipboardDomains.join('\n'))
const subLangsText = ref(form.value.subtitleLangs.join(', '))

function saveDomains() {
  form.value.clipboardDomains = domainsText.value
    .split(/[\s,，]+/)
    .map((d) => d.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, '').replace(/^\*?\./, ''))
    .filter((d) => d.includes('.'))
  domainsText.value = form.value.clipboardDomains.join('\n')
}
function saveSubLangs() {
  form.value.subtitleLangs = subLangsText.value
    .split(/[,，\s]+/)
    .map((l) => l.trim())
    .filter(Boolean)
  subLangsText.value = form.value.subtitleLangs.join(', ')
}
</script>

<template>
  <BalancedGrid class="groups" :mode="layout">
          <section class="group card">
            <h3>剪贴板</h3>
            <CcField label="监听剪贴板">
              <template #default="{ id }"><el-switch :id="id" v-model="form.watchClipboard" /></template>
            </CcField>
            <CcField label="识别后自动下载" desc="按默认选项直接加入队列，不弹出确认；保存位置和文件命名同“下载 → 保存”">
              <template #default="{ id }"><el-switch :id="id" v-model="form.autoDownload" :disabled="!form.watchClipboard" /></template>
            </CcField>
            <CcField label="全局快捷键（解析剪贴板）" layout="col" desc="按下后解析剪贴板里的链接；留空表示不使用。">
              <template #default="{ id }"><HotkeyInput :id="id" v-model="form.shortcut" label="解析剪贴板的全局快捷键" :others="others('shortcut')" :error="app.shortcutError" /></template>
            </CcField>
            <CcField label="截图翻译快捷键（仅 Windows）" layout="col" desc="按下后整屏冻结，框选要翻译的区域，用视觉模型识字翻译并盖回原位。需要在“AI”里选一个能看图的模型；留空表示不使用。">
              <template #default="{ id }"><HotkeyInput :id="id" v-model="form.shotHotkey" label="截图翻译快捷键" ctrl="Ctrl" :others="others('shotHotkey')" /></template>
            </CcField>
            <CcField label="翻译后自动复制译文">
              <template #default="{ id }"><el-switch :id="id" v-model="form.shotAutoCopy" /></template>
            </CcField>
            <CcField label="记录最近的截图翻译">
              <template #desc>只存文字，不存图片。在 <CcLink @click="app.view = 'screen'">屏幕翻译 → 翻译历史</CcLink> 里和实时翻译的历史一起看</template>
              <template #default="{ id }"><el-switch :id="id" v-model="form.shotHistory" /></template>
            </CcField>
            <CcField label="识别所有网址" desc="关闭时只识别内置平台和下面列出的网站，避免复制普通网址时频繁弹窗">
              <template #default="{ id }"><el-switch :id="id" v-model="form.clipboardAllSites" /></template>
            </CcField>
            <div v-if="!form.clipboardAllSites" class="field">
              <label>额外识别的网站</label>
              <el-input v-model="domainsText" type="textarea" :rows="3" size="small" class="mono" placeholder="youtube.com" @change="saveDomains" />
              <small class="mute">每行一个域名，同时匹配子域名。</small>
            </div>
          </section>
          <section class="group card">
            <h3>可选组件</h3>
            <CcField label="微信公众号文章视频" desc="默认关闭。开启后可以解析 mp.weixin.qq.com 文章里的微信原生视频（各清晰度直链，下载自动带微信 Referer）。视频号暂不支持。请只下载你有权保存的内容。">
              <template #default="{ id }"><el-switch :id="id" v-model="form.wechatOn" /></template>
            </CcField>
          </section>
          <section class="group card">
            <h3>收到的链接</h3>
            <small class="mute">手机、浏览器扩展发来的链接和手动解析失败的链接会记录在“媒体库 → 收到的链接”，可以随时重试。</small>
            <CcField label="记录剪贴板识别到的链接" desc="关闭后不再记录剪贴板里的链接">
              <template #default="{ id }"><el-switch :id="id" v-model="form.inbox.recordClipboard" /></template>
            </CcField>
            <CcField label="保留天数" desc="0 表示不按时间清理；进行中的不会被清理">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.inbox.keepDays" :min="0" :max="3650" size="small" /></template>
            </CcField>
            <CcField label="最多保留条数" desc="0 表示不限">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.inbox.maxItems" :min="0" :max="100000" :step="100" size="small" /></template>
            </CcField>
          </section>
          <section class="group card">
            <h3>字幕与弹幕</h3>
            <small class="mute">解析结果里会列出字幕轨：人工上传的字幕、YouTube 的自动生成字幕和偏好语言的自动翻译、B站的 CC / AI 字幕（需要登录）和弹幕。</small>
            <div class="field">
              <label>偏好语言（靠前的优先）</label>
              <el-input v-model="subLangsText" size="small" class="mono" placeholder="zh, en" @change="saveSubLangs" />
              <small class="mute">用逗号分隔，如 <span class="mono">zh, en, ja</span>。<span class="mono">zh</span> 同时匹配简体、繁体等变体。偏好语言的字幕在列表里排前面，也是“自动选中”的依据。</small>
            </div>
            <CcField label="下载视频时同时处理字幕" desc="对剪贴板自动下载、播放列表和订阅生效；解析页里也会默认选中偏好语言的字幕">
              <template #default="{ id }"><CcSelect :id="id" v-model="form.subtitleMode" :options="SUBTITLE_OPTS" width="m" /></template>
            </CcField>
            <CcField label="包含自动生成和自动翻译的字幕" desc="YouTube 没有人工字幕时很有用，质量不如人工字幕">
              <template #default="{ id }"><el-switch :id="id" v-model="form.subtitleAuto" /></template>
            </CcField>
            <CcField label="VTT 字幕转成 SRT" desc="SRT 是播放器和剪辑软件通用的格式；自动字幕的重复滚动文字也会一并整理">
              <template #default="{ id }"><el-switch :id="id" v-model="form.subtitleConvert" /></template>
            </CcField>
            <CcField label="B站弹幕转成 ASS" desc="播放器（PotPlayer、VLC、mpv 等）可以直接显示滚动弹幕；关闭则保存 XML 原文">
              <template #default="{ id }"><el-switch :id="id" v-model="form.danmakuAss" /></template>
            </CcField>
            <template v-if="form.danmakuAss">
              <CcField label="弹幕字号" desc="以 1080 高度的画面为准">
                <template #default="{ id }"><el-input-number :id="id" v-model="form.danmaku.fontSize" size="small" :min="18" :max="80" controls-position="right" /></template>
              </CcField>
              <CcField label="弹幕不透明度（%）">
                <template #default="{ id }"><el-slider :id="id" v-model="form.danmaku.opacity" :min="20" :max="100" :step="5" size="small" class="slim" /></template>
              </CcField>
              <CcField label="滚动一屏的时间（秒）" desc="越小越快">
                <template #default="{ id }"><el-input-number :id="id" v-model="form.danmaku.scrollSecs" size="small" :min="3" :max="20" controls-position="right" /></template>
              </CcField>
              <CcField label="弹幕占画面高度（%）" desc="留出底部给字幕">
                <template #default="{ id }"><el-slider :id="id" v-model="form.danmaku.area" :min="10" :max="100" :step="5" size="small" class="slim" /></template>
              </CcField>
              <CcField label="弹幕字体" layout="col">
                <template #default="{ id }"><el-input :id="id" v-model="form.danmaku.font" size="small" placeholder="留空使用微软雅黑（没有时播放器自动换用其他中文字体）" /></template>
              </CcField>
            </template>
          </section>
          <section class="group card">
            <h3>解析方式</h3>
            <CcField label="解析模式" layout="col">
              <template #default="{ id }"><CcSelect :id="id" v-model="form.parseMode" :options="PARSE_MODE_OPTS" width="full" /></template>
            </CcField>
            <CcField label="同一网站两次解析的最小间隔（毫秒）" desc="批量解析时避免请求过快被风控">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.siteRequestIntervalMs" :min="0" :max="10000" :step="100" size="small" /></template>
            </CcField>
            <CcField label="远程 API 地址" layout="col" desc="填写你自己部署的解析接口地址，只用于抖音、快手；其他平台始终本地解析。留空则不使用。">
              <template #default="{ id }"><el-input :id="id" v-model="form.remoteEndpoint" size="small" :placeholder="REMOTE_PH" class="mono" /></template>
            </CcField>
          </section>
          <section class="group card">
            <h3>清晰度与格式</h3>
            <CcField label="默认清晰度" desc="解析后默认选中，剪贴板自动下载和批量下载也按这里选择">
              <template #default="{ id }"><CcSelect :id="id" v-model="form.qualityPreset" :options="QUALITY_OPTS" width="m" /></template>
            </CcField>
            <CcField label="大小上限（MB）" desc="默认选中的格式预估超过上限时，改选不超过的最高清晰度；0 表示不限制。网站没给出大小时按码率和时长估算">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.maxSizeMb" size="small" :min="0" :max="1000000" :step="50" controls-position="right" /></template>
            </CcField>
            <CcField label="失败时自动降级" desc="网络、文件损坏等原因下载失败时，自动改用低一档的清晰度重新下载（最多 3 次）">
              <template #default="{ id }"><el-switch :id="id" v-model="form.autoDowngrade" /></template>
            </CcField>
            <CcField label="同等清晰度优先 H.264" desc="兼容性最好；AV1、H.265 体积更小，但老设备可能播不了">
              <template #default="{ id }"><el-switch :id="id" v-model="form.preferH264" /></template>
            </CcField>
            <CcField label="只要音频时的格式">
              <template #default="{ id }"><CcSelect :id="id" v-model="form.audioFormat" :options="AUDIO_OPTS" width="m" /></template>
            </CcField>
            <div class="field">
              <label>剧集 / 合集命名</label>
              <el-input v-model="form.seriesTemplate" size="small" class="mono" placeholder="{series}/第{episode}集" />
              <small class="mute">变量：{series} 剧名或列表名、{season} 季、{episode} 集数，以及 {title} {author} 等；“/” 表示子文件夹。留空则使用普通文件命名。</small>
            </div>
          </section>
          <SiteRulesPanel v-model="form.customSites" />
          <section class="group card">
            <h3>其他网站</h3>
            <CcField label="使用 yt-dlp 解析其他网站" desc="支持 YouTube、Pornhub、Twitter/X 等上千个网站；内置解析失效时也会用它重试">
              <template #default="{ id }"><el-switch :id="id" v-model="form.useYtdlp" /></template>
            </CcField>
            <CcField label="在网页中查找视频地址" desc="yt-dlp 也不支持时，尝试从网页里找 mp4 / m3u8 地址">
              <template #default="{ id }"><el-switch :id="id" v-model="form.genericSniffer" /></template>
            </CcField>
            <CcField label="m3u8 分片并发数">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.hlsConcurrency" :min="1" :max="32" size="small" /></template>
            </CcField>
            <CcField label="跳过 m3u8 里的插播广告" desc="按分片来源判断，极少数情况下可能误删正片片段">
              <template #default="{ id }"><el-switch :id="id" v-model="form.hlsSkipAds" /></template>
            </CcField>
          </section>
  </BalancedGrid>
</template>

<style scoped src="./settings-shared.css"></style>
