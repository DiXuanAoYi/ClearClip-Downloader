<script setup lang="ts">
import { computed } from 'vue'
import { useAppStore } from '../stores/app'
import type { WritingSettings } from '../types'

/** 设置 → 写作：桌面端的写作快捷键、自动浮标、不出现的程序、模型路线、个人风格。 */
const form = defineModel<WritingSettings>({ required: true })
const app = useAppStore()
const offText = computed({
  get: () => form.value.offApps.join('\n'),
  set: (v: string) => (form.value.offApps = v.split(/[\n,，;；]/).map((s) => s.trim()).filter(Boolean)),
})
const paused = computed(() => form.value.pausedUntil * 1000 > Date.now())
</script>

<template>
  <div class="groups">
    <section class="group card">
      <h3>写作浮窗（其他程序里）</h3>
      <div class="kv">
        <span>全局快捷键<small class="mute block">在任何程序里选中文字后按它，弹出写作窗。例如 Ctrl+Alt+J，留空表示不启用。网页里也可以用浏览器扩展的浮标。</small></span>
        <el-input v-model="form.hotkey" size="small" class="slim mono" placeholder="Ctrl+Alt+J" />
      </div>
      <small v-if="app.shortcutError" class="err">{{ app.shortcutError }}</small>
      <div class="kv">
        <span>点进输入框时出现小浮标<small class="mute block">仅 Windows。浏览器里由扩展负责，这里不会重复出现。</small></span>
        <el-switch v-model="form.badge" />
      </div>
      <div v-if="paused" class="kv">
        <span class="mute">已暂停，到点自动恢复</span>
        <el-button size="small" @click="form.pausedUntil = 0">立即恢复</el-button>
      </div>
      <div class="kv stack">
        <span>不在这些程序里出现<small class="mute block">每行一个进程名，如 code.exe。密码框、全屏程序、清影自己的窗口始终不出现。</small></span>
        <el-input v-model="offText" type="textarea" :rows="3" class="mono" placeholder="code.exe" />
      </div>
    </section>

    <section class="group card">
      <h3>模型与风格</h3>
      <div class="kv">
        <span>模型路线<small class="mute block">默认本地模型优先，没有再用云端 API。</small></span>
        <el-select v-model="form.route" size="small" class="slim">
          <el-option value="local_first" label="本地优先，云端备用" />
          <el-option value="local_only" label="只用本地模型" />
          <el-option value="cloud_only" label="只用云端 API" />
        </el-select>
      </div>
      <div class="kv">
        <span>一次给几个版本</span>
        <el-input-number v-model="form.candidates" :min="1" :max="3" size="small" />
      </div>
      <div class="kv stack">
        <span>我的写作风格<small class="mute block">一两句话，会附在每次改写的要求里。例如：口语化，短句，少用感叹号。仅保存在本机。</small></span>
        <el-input v-model="form.style" type="textarea" :rows="3" maxlength="300" show-word-limit />
      </div>
    </section>
  </div>
</template>

<style scoped>
.groups { display: grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap: 12px; align-items: start; }
.group { padding: 14px 16px; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
h3 { margin: 0; font-size: 12px; color: var(--cc-mute); font-weight: 500; letter-spacing: 0.08em; }
.kv { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
.kv > span:first-child { min-width: 0; }
.kv.stack { flex-direction: column; align-items: stretch; }
.block { display: block; font-size: 11px; margin-top: 2px; }
.slim { width: 210px; }
.err { color: var(--el-color-danger); }
</style>
