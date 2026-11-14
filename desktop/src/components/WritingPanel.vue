<script setup lang="ts">
import { computed, ref } from 'vue'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { WritingSettings } from '../types'
import { toastErr } from '../utils/toastErr'

/** 设置 → 写作：桌面端的写作快捷键、自动浮标、不出现的程序、模型路线、个人风格。 */
const form = defineModel<WritingSettings>({ required: true })
const app = useAppStore()
const offText = computed({
  get: () => form.value.offApps.join('\n'),
  set: (v: string) => (form.value.offApps = v.split(/[\n,，;；]/).map((s) => s.trim()).filter(Boolean)),
})
async function saveMemory() {
  try {
    await api.writeMemorySet(!!form.value.memory, form.value.memoryNotes ?? '')
  } catch (e) {
    learnMsg.value = errorText(e)
  }
}
async function forget(i: number) {
  await api.writeMemoryForget(i).catch(toastErr('忘记这条记忆'))
  form.value.memorySamples = (form.value.memorySamples ?? []).filter((_, k) => k !== i)
}
async function clearMemory() {
  await api.writeMemoryClear().catch(toastErr('清空记忆'))
  form.value.memoryNotes = ''
  form.value.memorySamples = []
}
const samples = ref('')
const learning = ref(false)
const learnMsg = ref('')

/** 把用户自己写的几段文字交给模型，总结成一两句话的“写作风格”。 */
async function learn() {
  if (!samples.value.trim() || learning.value) return
  learning.value = true
  learnMsg.value = '正在分析…'
  try {
    const id = await api.writeRun({ kind: 'style', text: samples.value, write: { action: 'rewrite', route: form.value.route } })
    for (;;) {
      await new Promise((r) => setTimeout(r, 500))
      const j = await api.writeJob(id)
      if (!j) throw new Error('任务已过期')
      if (j.state === 'error') throw new Error(j.error ?? '分析失败')
      if (j.state === 'done') {
        form.value.style = j.result?.style ?? form.value.style
        learnMsg.value = '已填入上面的“我的写作风格”，可以再改；记得点保存'
        break
      }
    }
  } catch (e) {
    learnMsg.value = errorText(e)
  } finally {
    learning.value = false
  }
}
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
      <div class="kv stack">
        <span>让清影学你的风格<small class="mute block">粘贴 1 到 3 段你自己写过的文字（聊天、邮件、帖子都行），模型总结出你的写作习惯。文字只发给你选的模型，不保存。</small></span>
        <el-input v-model="samples" type="textarea" :rows="4" placeholder="粘贴你自己写的文字" />
        <div class="kv">
          <el-button size="small" :loading="learning" :disabled="!samples.trim()" @click="learn">分析我的风格</el-button>
          <small class="mute">{{ learnMsg }}</small>
        </div>
      </div>
      <div class="kv">
        <span>写作记忆<small class="mute block">默认关闭。开了以后：记住你写回过的写法（最多 12 条），再加上下面“关于我”的笔记，每次一起交给你选的模型，让它更像你写的。记忆只存在本机，关掉后不再使用，可以随时清空。</small></span>
        <el-switch v-model="form.memory" @change="saveMemory" />
      </div>
      <template v-if="form.memory">
        <div class="kv stack">
          <span>关于我<small class="mute block">身份、常用称呼、落款、偏好。例如：我叫小林，产品经理；给客户用“您”；落款写“小林”。</small></span>
          <el-input v-model="form.memoryNotes" type="textarea" :rows="3" maxlength="1000" show-word-limit placeholder="只在相关时用，不会硬塞进每句话" @change="saveMemory" />
        </div>
        <div class="kv stack">
          <span>记下的写法（{{ (form.memorySamples ?? []).length }}）</span>
          <div v-if="!(form.memorySamples ?? []).length" class="mute">还没有。你在写作窗里点“替换原文”或“替换到…”后，这次写回去的文字会记在这里。</div>
          <div v-for="(m, i) in form.memorySamples ?? []" :key="i" class="mem">
            <span class="mute">{{ m.app || '—' }}</span>
            <span class="mtx">{{ m.text }}</span>
            <el-button size="small" text type="danger" @click="forget(i)">忘掉</el-button>
          </div>
          <el-button size="small" :disabled="!form.memoryNotes && !(form.memorySamples ?? []).length" @click="clearMemory">清空全部记忆</el-button>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.mem { display: grid; grid-template-columns: 90px 1fr auto; gap: 8px; align-items: baseline; font-size: 12.5px }
.mtx { word-break: break-word }
.groups { display: grid; grid-template-columns: repeat(auto-fit, minmax(380px, 1fr)); gap: 12px; align-items: start; }
.group { padding: 14px 16px; display: flex; flex-direction: column; gap: 12px; min-width: 0; }
h3 { margin: 0; font-size: 12px; color: var(--cc-mute); font-weight: 500; letter-spacing: 0.08em; }
.kv { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
.kv > span:first-child { min-width: 0; }
.kv.stack { flex-direction: column; align-items: stretch; }
.block { display: block; font-size: 11px; margin-top: 2px; }
.slim { width: 210px; }
.err { color: var(--cc-err); }
</style>
