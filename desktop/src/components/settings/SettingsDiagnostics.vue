<script setup lang="ts">
import type { Settings } from '../../types'
import type { LayoutMode } from '../../utils/balance'
import BalancedGrid from '../BalancedGrid.vue'
import CcButton from '../ui/CcButton.vue'
import CcField from '../ui/CcField.vue'
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import { healthAt, healthResults } from '../../utils/health'
import { copyDiagnostics, copyDiagnosticsSummary } from '../../composables/diagnostics'

const form = defineModel<Settings>({ required: true })
defineProps<{ layout: LayoutMode }>()

const health = healthResults
const checkingHealth = ref(false)

async function call(fn: () => Promise<unknown>) {
  try {
    await fn()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
/** 体检结果整理成文字复制出去：每行一个平台，带上链接和用时。示例链接去掉了查询参数（里面常有签名）。 */
async function copyHealth() {
  if (!health.value) return
  const lines = health.value.map((h) => {
    const mark = h.ok ? '正常' : h.sample ? '异常' : '未测'
    const url = h.sample ? `  ${h.sample.split(/[?#]/)[0]}` : ''
    const secs = h.millis ? `  ${(h.millis / 1000).toFixed(1)}s` : ''
    return `[${mark}] ${h.name}${secs}${url}\n    ${h.message}`
  })
  try {
    await api.copyText(`清影平台体检  ${new Date().toLocaleString()}\n${lines.join('\n')}`)
    ElMessage.success('体检结果已复制')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function runHealth() {
  checkingHealth.value = true
  health.value = null
  try {
    health.value = await api.healthCheck()
    healthAt.value = Date.now()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    checkingHealth.value = false
  }
}
</script>

<template>
  <BalancedGrid class="groups" :mode="layout">
          <section class="group card">
            <h3>问题反馈</h3>
            <p class="mute small">诊断信息包含版本、系统、网页组件和 yt-dlp / ffmpeg 的版本、设置概要、失败任务和最近日志（连续重复的行合成一行）。“复制摘要”只带警告和错误，短得多。Cookie、令牌等敏感内容会自动隐去。</p>
            <div class="inline wrap">
              <CcButton kind="primary" size="s" @click="copyDiagnostics">复制诊断信息</CcButton>
              <CcButton kind="secondary" size="s" @click="copyDiagnosticsSummary">复制摘要</CcButton>
              <CcButton kind="secondary" size="s" @click="call(api.openLogDir)">打开日志目录</CcButton>
            </div>
          </section>
          <section class="group card">
            <h3>解析样本</h3>
            <CcField label="保存解析时的原始响应" desc="用于排查网站改版导致的解析失败，平时不需要开启">
              <template #default="{ id }"><el-switch :id="id" v-model="form.recordSamples" /></template>
            </CcField>
            <p class="mute small">样本会去掉 Cookie、令牌和链接里的签名参数，但页面内容里仍可能有你的昵称等信息，分享前请检查。</p>
            <div class="inline"><CcButton kind="secondary" size="s" @click="call(api.openSamplesDir)">打开样本目录</CcButton></div>
          </section>
          <section class="group card wide">
            <h3>平台健康检查</h3>
            <p class="mute small">用示例链接（最近一次成功解析的链接，或内置示例）测试各平台解析是否正常，用于判断是网站改版还是网络问题。列表页（播放列表、创作者主页、合集）用你最近解析成功过的链接测，并且会展开其中一个列表试一次；没解析过的就显示“没有示例链接”。每升级一个版本后跑一次，有异常就复制结果发给开发者。</p>
            <div class="inline wrap">
              <CcButton kind="primary" size="s" :loading="checkingHealth" @click="runHealth">开始检查</CcButton>
              <CcButton kind="secondary" v-if="health" size="s" @click="copyHealth">复制结果</CcButton>
            </div>
            <div v-if="health" class="health">
              <div v-for="h in health" :key="h.platform" class="hrow">
                <span :class="h.ok ? 'okc' : h.sample ? 'err' : 'mute'">{{ h.ok ? '●' : h.sample ? '●' : '○' }}</span>
                <b>{{ h.name }}</b>
                <small class="ellipsis" :class="h.ok ? '' : h.sample ? 'err' : 'mute'" :title="h.message">{{ h.message }}</small>
                <small class="mono mute">{{ h.millis ? `${(h.millis / 1000).toFixed(1)}s` : '' }}</small>
              </div>
            </div>
          </section>
  </BalancedGrid>
</template>

<style scoped src="./settings-shared.css"></style>
