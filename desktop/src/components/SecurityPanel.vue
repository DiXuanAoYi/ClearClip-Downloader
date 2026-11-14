<script setup lang="ts">
import SafeboxView from '../views/SafeboxView.vue'
import CcButton from './ui/CcButton.vue'
import CcField from './ui/CcField.vue'
import CcDialog from './ui/CcDialog.vue'
import HotkeyInput from './HotkeyInput.vue'
import { computed, reactive, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { SecuritySettings, WipeOptions } from '../types'

const form = defineModel<SecuritySettings>({ required: true })
const app = useAppStore()
// 其他全局快捷键：老板键录入时查重
const otherKeys = computed(() => {
  const st = app.settings
  return st ? [{ name: '解析剪贴板', value: st.shortcut }, { name: '截图翻译', value: st.shotHotkey }, { name: '写作窗', value: st.writing.hotkey }] : []
})

// ---------- 应用锁 ----------
const dialog = ref<'set' | 'remove' | null>(null)
const current = ref('')
const pw = ref('')
const pw2 = ref('')
const busy = ref(false)
const pwMismatch = computed(() => !!pw2.value && pw.value !== pw2.value)

function openDialog(kind: 'set' | 'remove') {
  current.value = pw.value = pw2.value = ''
  dialog.value = kind
}

async function submitPassword() {
  busy.value = true
  try {
    if (dialog.value === 'set') {
      await api.lockSetPassword(pw.value, app.lock.enabled ? current.value : undefined)
      ElMessage.success(app.lock.enabled ? '密码已修改' : '应用锁已开启')
    } else {
      await api.lockRemovePassword(current.value)
      ElMessage.success('应用锁已关闭')
    }
    dialog.value = null
    await app.refreshLock()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

async function lockNow() {
  if (!(await api.lockNow())) ElMessage.info('还没有设置密码')
}

// ---------- 一键清除 ----------
const wipeOpen = ref(false)
const confirmText = ref('')
const opts = reactive<WipeOptions>({
  tasks: true,
  history: true,
  library: true,
  inbox: true,
  subscriptions: false,
  cookies: false,
  secrets: false,
  thumbnails: true,
  logs: true,
  safebox: false,
  quit: false,
})
const ITEMS: { key: keyof WipeOptions; label: string; hint: string }[] = [
  { key: 'tasks', label: '下载任务', hint: '进行中的任务会被取消，未完成的下载文件一并删除' },
  { key: 'history', label: '解析历史', hint: '“解析”页里的最近记录' },
  { key: 'library', label: '媒体库记录', hint: '标签、评分、字幕索引等。已下载的视频文件不会被删除，回收站里的文件也不动' },
  { key: 'inbox', label: '收到的链接', hint: '手机发送、剪贴板、手动解析的链接记录' },
  { key: 'subscriptions', label: '订阅与直播间', hint: '订阅列表、直播间列表和录制记录' },
  { key: 'cookies', label: '登录状态', hint: '保存的账号 Cookie 和登录窗口里的浏览数据；清除后需要重新登录' },
  { key: 'secrets', label: 'API 密钥和通知令牌', hint: 'AI 密钥、WebDAV 密码、机器人令牌；应用锁密码保留' },
  { key: 'thumbnails', label: '封面与预览图缓存', hint: '' },
  { key: 'logs', label: '日志与诊断样本', hint: '' },
  { key: 'safebox', label: '加密保险箱里的文件', hint: '永久删除，无法恢复' },
]
const picked = computed(() => ITEMS.filter((i) => opts[i.key]))
const canWipe = computed(() => picked.value.length > 0 && confirmText.value.trim() === '清除' && !busy.value)

function openWipe() {
  confirmText.value = ''
  wipeOpen.value = true
}

async function wipe() {
  if (opts.safebox) {
    try {
      await ElMessageBox.confirm('保险箱里的文件会被永久删除，忘记密码以外，这是唯一无法恢复的操作。确定？', '再确认一次', { type: 'warning', confirmButtonText: '删除保险箱', cancelButtonText: '返回' })
    } catch {
      return
    }
  }
  busy.value = true
  try {
    const r = await api.wipeTraces({ ...opts })
    wipeOpen.value = false
    ElMessage.success(`已清除：${r.done.join('、')}`)
    for (const s of r.skipped) ElMessage.warning(s)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="groups">
    <section class="group card">
      <h3>应用锁</h3>
      <CcField :label="app.lock.enabled ? '应用锁：已开启' : '应用锁：未开启'" desc="锁定后界面被遮住，托盘迷你窗和系统通知不显示内容。下载仍在后台继续，手机推送、命令行和网页控制台也照常工作。它挡的是坐在屏幕前的人，不会加密你的文件。" layout="col">
        <div class="btns">
          <CcButton v-if="!app.lock.enabled" kind="primary" size="s" @click="openDialog('set')">设置密码</CcButton>
          <template v-else>
            <CcButton size="s" @click="lockNow">立即锁定</CcButton>
            <CcButton size="s" @click="openDialog('set')">修改密码</CcButton>
            <CcButton size="s" @click="openDialog('remove')">关闭</CcButton>
          </template>
        </div>
      </CcField>
      <template v-if="app.lock.enabled">
        <CcField label="空闲自动锁定" desc="鼠标键盘一段时间没有动作后锁定，0 表示不自动锁定">
          <template #default="{ id }"><span class="unit"><el-input-number :id="id" v-model="form.autoLockMinutes" :min="0" :max="1440" size="small" /> 分钟</span></template>
        </CcField>
        <CcField label="收进托盘时锁定" desc="点关闭按钮或托盘“锁定”时">
          <template #default="{ id }"><el-switch :id="id" v-model="form.lockOnHide" /></template>
        </CcField>
      </template>
      <CcField label="老板键" layout="col" desc="按下立刻隐藏所有窗口，设置了应用锁时同时锁定。点一下输入框后直接按下组合键（例如 Ctrl + Shift + H），Esc 取消；留空表示不启用。">
        <template #default="{ id }"><HotkeyInput :id="id" v-model="form.panicShortcut" label="老板键" :others="otherKeys" /></template>
      </CcField>
    </section>

    <section class="group card">
      <h3>隐私模式</h3>
      <CcField label="不留下记录" desc="开启后：不记录解析历史；下载完成的文件不进媒体库，也不写 nfo / info.json；任务结束就从数据库删除；系统通知不显示标题；退出清影时清空收件箱。因此自动规则、自动上传和 AI 自动任务不会对这些文件运行。订阅和直播间列表是你主动保存的内容，不受影响。">
        <template #default="{ id }"><el-switch :id="id" v-model="form.privacyMode" /></template>
      </CcField>
      <CcField label="禁止截屏和录屏" desc="Windows 和 macOS 上，截图或录屏软件只能拍到黑屏；Linux 不支持">
        <template #default="{ id }"><el-switch :id="id" v-model="form.contentProtection" /></template>
      </CcField>
    </section>

    <section class="group card">
      <h3>加密保险箱</h3>
      <CcField label="把敏感文件加密存放" desc="AES-256 加密，文件名也隐藏，只有输入保险箱密码才能打开。忘记密码无法找回。">
        <span />
      </CcField>
      <SafeboxView embedded />
      <CcField label="空闲自动上锁" desc="保险箱一段时间没有操作后自动上锁，并清除解密出来的临时文件；0 表示不自动上锁">
        <template #default="{ id }"><span class="unit"><el-input-number :id="id" v-model="form.safeboxAutoLockMinutes" :min="0" :max="1440" size="small" /> 分钟</span></template>
      </CcField>
    </section>

    <section class="group card">
      <h3>一键清除</h3>
      <CcField label="清除使用痕迹" desc="按你勾选的项目删除记录，并压缩数据库让旧内容不再留在文件里。不会删除你下载的视频文件。">
        <CcButton kind="danger" size="s" @click="openWipe">清除…</CcButton>
      </CcField>
    </section>

    <CcDialog :model-value="dialog !== null" :title="dialog === 'set' ? (app.lock.enabled ? '修改应用锁密码' : '设置应用锁密码') : '关闭应用锁'" size="sm" @update:model-value="dialog = null">
      <div class="col">
        <el-input v-if="app.lock.enabled" v-model="current" type="password" show-password placeholder="当前密码" aria-label="当前密码" autocomplete="off" />
        <template v-if="dialog === 'set'">
          <el-input v-model="pw" type="password" show-password placeholder="新密码（至少 4 个字符）" aria-label="新密码" autocomplete="off" />
          <el-input v-model="pw2" type="password" show-password placeholder="再输入一次" aria-label="再输入一次新密码" autocomplete="off" />
          <div v-if="pwMismatch" class="err" role="alert">两次输入的密码不一致。</div>
          <small class="mute">忘记密码时，可以退出清影后删除数据目录里的 vault.bin 重置（会同时清掉保存的 API 密钥）。保险箱用另一个密码，不受影响。</small>
        </template>
      </div>
      <template #footer>
        <CcButton @click="dialog = null">取消</CcButton>
        <CcButton kind="primary" :loading="busy" :disabled="(app.lock.enabled && !current) || (dialog === 'set' && (pw.length < 4 || pw !== pw2))" @click="submitPassword">确定</CcButton>
      </template>
    </CcDialog>

    <CcDialog v-model="wipeOpen" title="一键清除" size="md" danger initial-focus="[data-cancel]">
      <div class="col wipe-body">
        <div v-for="i in ITEMS" :key="i.key" class="wipe-row">
          <el-checkbox v-model="opts[i.key]">{{ i.label }}</el-checkbox>
          <small v-if="i.hint" class="mute">{{ i.hint }}</small>
        </div>
        <el-checkbox v-model="opts.quit">清除后退出清影</el-checkbox>
      </div>
      <template #footer>
        <el-input v-model="confirmText" size="small" class="confirm-in" placeholder="输入“清除”确认" aria-label="输入“清除”确认" @keyup.enter="canWipe && wipe()" />
        <CcButton data-cancel @click="wipeOpen = false">取消</CcButton>
        <CcButton kind="danger" :loading="busy" :disabled="!canWipe" disabled-reason="先勾选要清除的项目，并输入“清除”" @click="wipe">清除 {{ picked.length }} 项</CcButton>
      </template>
    </CcDialog>
  </div>
</template>

<style scoped>
.groups {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
  gap: var(--cc-sp-3);
  align-items: start;
}
.group {
  padding: var(--cc-sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--cc-sp-3);
  min-width: 0;
}
h3 {
  margin: 0;
  font-size: var(--cc-fs-lg);
  color: var(--cc-fg);
  font-weight: 700;
}
.btns {
  display: flex;
  flex-wrap: wrap;
  gap: var(--cc-sp-2);
}
.unit {
  display: inline-flex;
  align-items: center;
  gap: var(--cc-sp-2);
  white-space: nowrap;
}
.col {
  display: flex;
  flex-direction: column;
  gap: var(--cc-sp-3);
}
.confirm-in {
  width: 160px;
  margin-right: var(--cc-sp-2);
}
.wipe-body {
  max-height: calc(100vh - 260px);
  overflow-y: auto;
  padding-right: var(--cc-sp-1);
}
.wipe-row {
  display: flex;
  flex-direction: column;
}
.wipe-row small {
  margin-left: 24px;
  font-size: var(--cc-fs-sm);
}
.err {
  color: var(--cc-err-text);
  font-size: var(--cc-fs-sm);
}
</style>
