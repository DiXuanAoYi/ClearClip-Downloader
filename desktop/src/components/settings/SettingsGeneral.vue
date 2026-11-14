<script setup lang="ts">
import type { Settings } from '../../types'
import type { LayoutMode } from '../../utils/balance'
import BalancedGrid from '../BalancedGrid.vue'
import CcButton from '../ui/CcButton.vue'
import CcField from '../ui/CcField.vue'
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import { useAppStore } from '../../stores/app'
import type { UpdateInfo } from '../../types'
import { LANGUAGES } from '../../i18n'
import CcSegmented from '../ui/CcSegmented.vue'
import CcLink from '../ui/CcLink.vue'

const app = useAppStore()
const form = defineModel<Settings>({ required: true })
// 设置页卡片的排布方式：只是这台电脑上的界面偏好，由 SettingsView 记在本机
const layout = defineModel<LayoutMode>('layout', { required: true })

const THEME_OPTS = [{ value: 'system', label: '跟随系统' }, { value: 'dark', label: '深色' }, { value: 'light', label: '浅色' }]
const LAYOUT_OPTS = [{ value: 'balance', label: '自动平衡' }, { value: 'order', label: '按顺序' }, { value: 'single', label: '单列' }]
const LANG_OPTS = LANGUAGES.map((l) => ({ value: l.value, label: l.label }))
const update = ref<UpdateInfo | null>(null)
const checking = ref(false)
const installing = ref(false)
const portable = ref(false)
api.isPortable().then((v) => (portable.value = v)).catch(() => {}) // 忽略：探测是否便携版，失败按否处理

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
</script>

<template>
  <BalancedGrid class="groups" :mode="layout">
          <section class="group card">
            <h3>外观与行为</h3>
            <CcField label="外观"><CcSegmented v-model="form.theme" :options="THEME_OPTS" size="s" aria-label="外观" /></CcField>
            <CcField label="设置页排布" desc="卡片怎么排：自动平衡会让各列一样高、不留空白；按顺序严格按先后放；单列最简单"><CcSegmented v-model="layout" :options="LAYOUT_OPTS" size="s" aria-label="设置页排布" /></CcField>
            <CcField label="界面语言" desc="English 界面由机器翻译整理，较长的提示和软件返回的错误消息仍可能是中文"><CcSegmented v-model="form.language" :options="LANG_OPTS" size="s" aria-label="界面语言" /></CcField>
            <CcField label="关闭窗口时最小化到托盘">
              <template #default="{ id }"><el-switch :id="id" v-model="form.closeToTray" /></template>
            </CcField>
            <CcField label="悬浮窗" desc="桌面上的清影小窗口：把浏览器里的链接拖进去就开始下载，也可以点一下后按 Ctrl+V 粘贴；双击回到主窗口。在“屏幕翻译”里开了“悬浮窗里放屏幕翻译控制”的话，这个窗里还有屏幕翻译的开关（不管这里开没开都会显示）">
              <template #default="{ id }"><el-switch :id="id" v-model="form.floatBall" /></template>
            </CcField>
            <CcField label="开机自动启动" desc="在托盘运行，用于订阅的定期检查">
              <template #default="{ id }"><el-switch :id="id" v-model="form.launchAtLogin" /></template>
            </CcField>
            <CcField label="订阅与追更" desc="在“订阅”页面管理">
              <template #default="{ id }"><el-switch :id="id" v-model="form.subscriptionsEnabled" /></template>
            </CcField>
            <CcField label="启动时检查更新">
              <template #default="{ id }"><el-switch :id="id" v-model="form.checkUpdate" /></template>
            </CcField>
            <small v-if="portable" class="mute">便携模式：设置、数据库和日志保存在程序目录下的 data 文件夹。</small>
          </section>
          <section class="group card">
            <h3>媒体库</h3>
            <CcField label="删除文件时先放进回收站" desc="在媒体库里删除文件时移到下载目录里的隐藏文件夹，可以还原">
              <template #default="{ id }"><el-switch :id="id" v-model="form.library.useTrash" /></template>
            </CcField>
            <CcField v-if="form.library.useTrash" label="回收站保留天数" desc="超过天数的文件在启动时永久删除，0 表示不自动清空">
              <template #default="{ id }"><el-input-number :id="id" v-model="form.library.trashKeepDays" size="small" :min="0" :max="3650" controls-position="right" /></template>
            </CcField>
            <CcField label="下载完成后记录时长并截取封面" desc="用 ffmpeg 读取视频时长；没有封面的视频会截一张图">
              <template #default="{ id }"><el-switch :id="id" v-model="form.library.probeNewFiles" /></template>
            </CcField>
            <CcField label="“整理文件夹”的默认规则" layout="col" desc="变量：{platform} {author} {year} {month} {day} {date} {kind}">
              <template #default="{ id }"><el-input :id="id" v-model="form.library.reorganizeTemplate" size="small" class="mono" placeholder="{platform}/{author}" /></template>
            </CcField>
          </section>
          <section class="group card">
            <h3>关于</h3>
            <div class="kv">
              <span>清影 ClearClip v{{ app.info?.version }}</span>
              <CcButton kind="secondary" size="s" :loading="checking" @click="checkUpdate">检查更新</CcButton>
            </div>
            <div v-if="update?.hasUpdate" class="kv">
              <span>新版本 v{{ update.latest }} 可用</span>
              <CcButton v-if="update.canInstall" kind="primary" size="s" :loading="installing" @click="installUpdate">立即更新并重启</CcButton>
              <CcButton v-else kind="primary" size="s" @click="api.openUrl(update.url)">前往下载</CcButton>
            </div>
            <p class="mute small">
              仅用于下载你有权保存的内容。项目地址：<CcLink :href="`https://github.com/${app.info?.repo}`" external>github.com/{{ app.info?.repo }}</CcLink>
            </p>
          </section>
  </BalancedGrid>
</template>

<style scoped src="./settings-shared.css"></style>
