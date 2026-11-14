<script setup lang="ts">
import type { Settings } from '../../types'
import type { LayoutMode } from '../../utils/balance'
import BalancedGrid from '../BalancedGrid.vue'
import CcButton from '../ui/CcButton.vue'
import CcField from '../ui/CcField.vue'
import CcSelect from '../ui/CcSelect.vue'
import { computed } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { previewFilename } from '../../utils/format'

const form = defineModel<Settings>({ required: true })
defineProps<{ layout: LayoutMode }>()

const CONFLICT_OPTS = [{ value: 'rename', label: '自动重命名' }, { value: 'skip', label: '跳过' }, { value: 'overwrite', label: '覆盖' }]
const MERGE_OPTS = [{ value: 'mp4', label: 'MP4' }, { value: 'mkv', label: 'MKV' }]
const DAYS = ['一', '二', '三', '四', '五', '六', '日']

function addSpeedWindow() {
  form.value.speedSchedule.push({ days: [], start: '08:00', end: '23:00', limitKbps: 1024 })
}
async function pickDir() {
  const dir = await open({ directory: true, defaultPath: form.value.downloadDir || undefined, title: '选择保存位置' })
  if (typeof dir === 'string') form.value.downloadDir = dir
}
async function pickTempDir() {
  const dir = await open({ directory: true, defaultPath: form.value.tempDir || undefined, title: '选择临时文件目录' })
  if (typeof dir === 'string') form.value.tempDir = dir
}
function insertVar(v: string) {
  form.value.filenameTemplate += v
}
const nameSample = { author: '山野厨房', title: '秋天第一锅板栗焖鸡 #家常菜', id: '7421234567890123456', platform: '抖音', quality: '1080P' }
const namePreview = computed(() => previewFilename(form.value.filenameTemplate, nameSample))
/** 模板里不认识的 {变量}：会原样写进文件名，提前提醒。 */
const NAME_VARS = ['author', 'title', 'date', 'id', 'platform', 'quality']
const unknownVars = computed(() => [...new Set([...form.value.filenameTemplate.matchAll(/\{([^{}]*)\}/g)].map((m) => m[1]).filter((v) => !NAME_VARS.includes(v)))])
const unknownText = computed(() => unknownVars.value.map((v) => `{${v}}`).join('、'))
</script>

<template>
  <BalancedGrid class="groups" :mode="layout">
          <section class="group card">
            <h3>保存</h3>
            <CcField label="保存位置" layout="col">
              <div class="inline">
                <el-input v-model="form.downloadDir" size="small" class="grow" />
                <CcButton kind="secondary" size="s" @click="pickDir">选择…</CcButton>
              </div>
            </CcField>
            <CcField label="按平台分子文件夹">
              <template #default="{ id }"><el-switch :id="id" v-model="form.subfolderByPlatform" /></template>
            </CcField>
            <CcField label="文件命名" layout="col" desc="手动下载、识别后自动下载、手机发送、订阅都按这个命名，下载队列和完成通知里显示的也是这个名字。">
              <div class="namebox">
                <el-input v-model="form.filenameTemplate" size="small" class="mono" aria-label="文件命名模板" />
                <div class="vars" role="group" aria-label="插入变量">
                  <button v-for="v in ['{author}', '{title}', '{date}', '{id}', '{platform}', '{quality}']" :key="v" type="button" class="var mono" @click="insertVar(v)">{{ v }}</button>
                </div>
                <small class="mute ellipsis">示例：{{ namePreview }}</small>
                <small v-if="unknownVars.length" class="warn" role="status">
                  <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M7 1.5 13 12H1z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" /><path d="M7 5.6v3M7 10.2v.1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
                  不认识的变量 {{ unknownText }}，会原样写进文件名。
                </small>
              </div>
            </CcField>
            <CcField label="跳过已下载过的内容">
              <template #default="{ id }"><el-switch :id="id" v-model="form.skipExisting" /></template>
            </CcField>
            <CcField label="目标文件已存在时">
              <template #default="{ id }"><CcSelect :id="id" v-model="form.conflictPolicy" :options="CONFLICT_OPTS" width="m" /></template>
            </CcField>
            <CcField label="临时文件目录" layout="col" desc="可指定到空间更大的磁盘。合并音视频时需要约两倍的临时空间。">
              <div class="inline">
                <el-input v-model="form.tempDir" size="small" class="grow" placeholder="留空表示与保存位置相同" />
                <CcButton kind="secondary" size="s" @click="pickTempDir">选择…</CcButton>
              </div>
            </CcField>
            <CcField label="保留的磁盘剩余空间（MB）" desc="空间不足时暂停下载并提示">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.diskReserveMb" :min="0" :max="102400" :step="100" size="small" /></template>
            </CcField>
            <CcField label="全部下载完成后发送通知">
              <template #default="{ id }"><el-switch :id="id" v-model="form.notifyOnComplete" /></template>
            </CcField>
          </section>

          <section class="group card">
            <h3>队列与断点续传</h3>
            <CcField label="同时下载数">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.concurrency" :min="1" :max="8" size="small" /></template>
            </CcField>
            <CcField label="每个网站同时下载数" desc="使用登录账号的网站自动减半，降低被限制的风险">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.perSiteConcurrency" :min="1" :max="8" size="small" /></template>
            </CcField>
            <CcField label="大文件分段数" desc="服务器支持时多线程并行下载，1 表示不分段">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.segments" :min="1" :max="64" size="small" /></template>
            </CcField>
            <CcField label="文件大于多少 MB 时分段">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.segmentMinMb" :min="2" :max="1024" size="small" /></template>
            </CcField>
            <CcField label="全局限速（KB/s）" desc="0 表示不限速">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.speedLimitKbps" :min="0" :max="1048576" :step="256" size="small" /></template>
            </CcField>
            <CcField label="启动时自动继续未完成的任务" desc="关闭时恢复为“已暂停”，手动继续">
              <template #default="{ id }"><el-switch :id="id" v-model="form.autoResume" /></template>
            </CcField>
            <CcField label="网络中断时自动重试">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.maxRetries" :min="0" :max="10" size="small" /></template>
            </CcField>
            <CcField label="第一次重试前等待（秒）" desc="之后每次等待时间翻倍">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.retryDelaySecs" :min="1" :max="60" size="small" /></template>
            </CcField>
            <CcField label="取消任务时保留已下载部分" desc="保留后重新加入同一资源可接着下载">
              <template #default="{ id }"><el-switch :id="id" v-model="form.keepPartOnCancel" /></template>
            </CcField>
            <small class="mute">关闭程序、断网或下载地址过期后，任务都会从已下载的位置继续；服务器不支持续传时会自动从头下载。</small>
          </section>

          <section class="group card">
            <h3>批量下载</h3>
            <CcField label="批量添加时先预览" desc="先解析每条链接，标出重复、已下载过、需要登录的，再确定要下载哪些；关闭后直接按默认选项整批加入">
              <template #default="{ id }"><el-switch :id="id" v-model="form.batchPreview" /></template>
            </CcField>
            <CcField label="几条起默认放进批次文件夹" desc="用批次名当文件夹名">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.batchFolderMin" :min="1" :max="500" size="small" /></template>
            </CcField>
            <CcField label="同一网站相邻两条的随机间隔（秒）" desc="避免连续请求被网站限制"><span style="display:flex;gap:6px;align-items:center">
                <el-input-number v-model="form.batchJitterMin" :min="0" :max="form.batchJitterMax" size="small" />
                <span class="mute small">—</span>
                <el-input-number v-model="form.batchJitterMax" :min="form.batchJitterMin" :max="120" size="small" />
              </span></CcField>
            <CcField label="启用“只在设定时段下载”" desc="批量添加时勾选“只在设定时段下载”的批次，时段外暂停（限速填 0）或按下面的限速继续">
              <template #default="{ id }"><el-switch :id="id" v-model="form.runWindowOn" /></template>
            </CcField>
            <div v-if="form.runWindowOn" class="win">
              <el-checkbox-group v-model="form.runWindow.days" size="small">
                <el-checkbox-button v-for="(d, k) in DAYS" :key="k" :value="k + 1">{{ d }}</el-checkbox-button>
              </el-checkbox-group>
              <el-time-select v-model="form.runWindow.start" size="small" class="tsel" start="00:00" end="23:30" step="00:30" :clearable="false" />
              <span class="mute small">到</span>
              <el-time-select v-model="form.runWindow.end" size="small" class="tsel" start="00:00" end="23:30" step="00:30" :clearable="false" />
              <span class="mute small">时段外限速</span>
              <el-input-number v-model="form.runWindow.limitKbps" :min="0" :max="1048576" :step="256" size="small" class="lim" />
              <span class="mute small">KB/s（0 = 暂停）</span>
            </div>
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
                <CcButton kind="text" size="s" @click="form.speedSchedule.splice(i, 1)">删除</CcButton>
              </div>
              <div>
                <CcButton kind="secondary" size="s" @click="addSpeedWindow">添加时段</CcButton>
                <span class="mute small"> {{ form.speedSchedule.length ? '限速填 0 表示这个时段不限速；不选星期表示每天；结束早于开始表示跨过午夜' : '例如白天限速、半夜不限速' }}</span>
              </div>
            </div>
            <CcField label="省流量模式" desc="用于手机热点等按流量计费的网络：下载并发降为 1、分段数减少、默认画质改为省空间（不超过 720p）、不自动下载订阅更新、不自动上传和调用 AI，已排队的任务不受影响">
              <template #default="{ id }"><el-switch :id="id" v-model="form.meteredMode" /></template>
            </CcField>
          </section>

          <section class="group card">
            <h3>整理</h3>
            <CcField label="把标题、作者、封面写入文件" desc="需要 ffmpeg；视频和音频在播放器、音乐软件里能显示信息">
              <template #default="{ id }"><el-switch :id="id" v-model="form.embedMetadata" /></template>
            </CcField>
            <CcField label="保存作品信息 JSON" desc="标题、作者、原链接、发布时间等，与视频同名">
              <template #default="{ id }"><el-switch :id="id" v-model="form.writeInfoJson" /></template>
            </CcField>
            <CcField label="生成 NFO 文件" desc="Jellyfin、Kodi、Plex 等媒体服务器能识别">
              <template #default="{ id }"><el-switch :id="id" v-model="form.writeNfo" /></template>
            </CcField>
            <CcField label="音视频合并后的格式" desc="MKV 兼容的编码更多，MP4 播放兼容性更好">
              <template #default="{ id }"><CcSelect :id="id" v-model="form.mergeContainer" :options="MERGE_OPTS" width="m" /></template>
            </CcField>
          </section>

          <section class="group card">
            <h3>完成后</h3>
            <CcField label="下载后检查文件" desc="用 ffmpeg 检查视频能否正常读取；损坏时自动清理并重新下载一次。需要安装 ffmpeg">
              <template #default="{ id }"><el-switch :id="id" v-model="form.verifyDownloads" /></template>
            </CcField>
            <CcField label="全部完成后打开下载文件夹">
              <template #default="{ id }"><el-switch :id="id" v-model="form.openFolderOnDone" /></template>
            </CcField>
            <CcField label="有下载任务时阻止系统休眠">
              <template #default="{ id }"><el-switch :id="id" v-model="form.preventSleep" /></template>
            </CcField>
            <CcField label="每个任务完成后运行命令" desc="高级功能，只运行你自己填写的命令">
              <template #default="{ id }"><el-switch :id="id" v-model="form.postScriptEnabled" /></template>
            </CcField>
            <div v-if="form.postScriptEnabled" class="field">
              <el-input v-model="form.postScript" size="small" class="mono" placeholder='例如：python "D:\tools\upload.py"' />
              <small class="mute">文件路径等信息通过环境变量传入：CLEARCLIP_FILE、CLEARCLIP_TITLE、CLEARCLIP_AUTHOR、CLEARCLIP_URL、CLEARCLIP_PLATFORM。</small>
            </div>
            <small class="mute">“全部完成后睡眠 / 关机”在下载队列页面设置，只对本次运行有效。</small>
          </section>
  </BalancedGrid>
</template>

<style scoped src="./settings-shared.css"></style>
