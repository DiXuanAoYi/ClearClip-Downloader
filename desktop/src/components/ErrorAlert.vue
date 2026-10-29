<script setup lang="ts">
import { computed } from 'vue'
import { useAppStore } from '../stores/app'
import type { ErrorKind } from '../types'
import { copyDiagnostics } from '../composables/diagnostics'
import { guideFor, siteName } from '../utils/siteGuides'

/** site：出错链接对应的网站（内置平台 ID 或域名），用于“登录”按钮直接打开该网站的登录 */
const props = defineProps<{ message: string; kind?: ErrorKind | null; compact?: boolean; site?: string | null }>()
const app = useAppStore()

const login = computed(() => {
  if (!props.site) return { action: '添加 Cookie', run: () => app.goSettings('accounts'), tip: '' }
  const name = siteName(props.site, app.info?.providers)
  const g = guideFor(props.site)
  const how = g.embeddedBlocked ? `${name}会拦截内置登录窗口，需要用浏览器扩展同步或导入 cookies.txt。` : ''
  return { action: `登录${name}`, run: () => app.goLogin(props.site), tip: `${g.needLogin ? `${name}：${g.needLogin}` : ''}${how}` }
})

/** 每类错误的说明和下一步操作。 */
const hint = computed(() => {
  switch (props.kind) {
    case 'need_login':
      return { tip: `这个内容需要登录，登录后会自动重试。${login.value.tip}`, action: login.value.action, run: login.value.run }
    case 'geo_blocked':
      return { tip: '当前网络无法访问这个内容，可能有地区限制。请检查这个网站的代理规则。', action: '网络设置', run: () => app.goSettings('network') }
    case 'network':
      return { tip: '网络连接失败。请检查网络，或在网络设置里测试这个网站能否访问。', action: '网络设置', run: () => app.goSettings('network') }
    case 'rate_limited':
      return { tip: '请求太频繁，被网站暂时限制。请过几分钟再试，或登录后重试。', action: login.value.action, run: login.value.run }
    case 'encrypted':
      return { tip: '内容已加密（DRM 或付费加密），清影不支持下载这类内容。', action: null, run: null }
    case 'not_found':
      return { tip: '作品可能已删除、设为私密，或链接已失效。', action: null, run: null }
    case 'parser_broken':
      return { tip: '网站可能改版了，解析器需要更新。复制诊断信息反馈给开发者会很有帮助。', action: '复制诊断信息', run: copyDiagnostics }
    case 'unsupported':
      return { tip: '暂不支持这个网站或链接类型。', action: null, run: null }
    case 'need_update':
      return { tip: '需要安装或更新组件（yt-dlp / ffmpeg）后才能继续。', action: '管理组件', run: () => app.goSettings('components') }
    case 'disk':
      return { tip: '磁盘空间不足或没有写入权限。请检查下载目录。', action: '下载设置', run: () => app.goSettings('download') }
    default:
      return null
  }
})
</script>

<template>
  <el-alert :type="kind === 'encrypted' || kind === 'unsupported' ? 'warning' : 'error'" show-icon :closable="false" class="err-alert">
    <template #title>
      <span class="selectable">{{ message }}</span>
    </template>
    <div v-if="hint && (!compact || (kind === 'need_login' && site))" class="hint">
      <span>{{ hint.tip }}</span>
      <el-button v-if="hint.action && hint.run" size="small" @click="hint.run()">{{ hint.action }}</el-button>
    </div>
  </el-alert>
</template>

<style scoped>
.hint {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
  margin-top: 4px;
}
.err-alert :deep(.el-alert__content) {
  min-width: 0;
}
</style>
