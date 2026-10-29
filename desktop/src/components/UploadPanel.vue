<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { UploadSettings } from '../types'

const up = defineModel<UploadSettings>({ required: true })
const pass = ref('')
const hasPass = ref(false)
const testing = ref(false)

const KINDS = [
  { v: 'video', t: '视频' },
  { v: 'audio', t: '音频' },
  { v: 'image', t: '图片' },
  { v: 'subtitle', t: '字幕' },
  { v: 'cover', t: '封面' },
]

onMounted(async () => {
  hasPass.value = await api.secretHas('upload.webdav').catch(() => false)
})

async function savePass() {
  try {
    await api.secretSet('upload.webdav', pass.value)
    pass.value = ''
    hasPass.value = await api.secretHas('upload.webdav')
    ElMessage.success('密码已加密保存')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function pickDir() {
  const d = await open({ directory: true, multiple: false })
  if (typeof d === 'string') up.value.url = d
}

async function test() {
  testing.value = true
  try {
    await new Promise((r) => setTimeout(r, 700))
    ElMessage.success(await api.uploadTest())
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <section class="group card">
    <h3>自动上传 / 备份到网盘</h3>
    <small class="mute">每个文件下载完成后，自动上传到 WebDAV（Nextcloud、坚果云、Alist、群晖等），或复制到另一个文件夹（NAS 挂载盘、移动硬盘）。进度在“工具箱”的任务列表里。</small>
    <div class="kv"><span>启用</span><el-switch v-model="up.enabled" /></div>
    <template v-if="up.enabled">
      <div class="kv">
        <span>方式</span>
        <el-radio-group v-model="up.kind" size="small">
          <el-radio-button value="webdav">WebDAV</el-radio-button>
          <el-radio-button value="folder">复制到文件夹</el-radio-button>
        </el-radio-group>
      </div>
      <div class="field">
        <label>{{ up.kind === 'webdav' ? 'WebDAV 地址' : '目标文件夹' }}</label>
        <div class="row">
          <el-input v-model="up.url" size="small" class="mono" :placeholder="up.kind === 'webdav' ? 'https://dav.example.com/remote.php/dav/files/用户名/' : '例如 \\\\NAS\\video 或 /Volumes/NAS'" />
          <el-button v-if="up.kind === 'folder'" size="small" @click="pickDir">选择…</el-button>
        </div>
      </div>
      <template v-if="up.kind === 'webdav'">
        <div class="field"><label>用户名</label><el-input v-model="up.user" size="small" /></div>
        <div class="field">
          <label>密码 / 应用专用密码 <span v-if="hasPass" class="ok">已保存</span></label>
          <div class="row">
            <el-input v-model="pass" size="small" type="password" show-password :placeholder="hasPass ? '输入新密码可替换' : '密码'" />
            <el-button size="small" :disabled="!pass" @click="savePass">保存</el-button>
          </div>
        </div>
      </template>
      <div class="field">
        <label>保存到目标下的子目录</label>
        <el-input v-model="up.remoteDir" size="small" class="mono" placeholder="ClearClip/{platform}/{author}" />
        <small class="mute">变量：{platform} {author} {year} {month} {day} {date} {kind}</small>
      </div>
      <div class="field">
        <label>上传哪些文件</label>
        <el-checkbox-group v-model="up.kinds" size="small">
          <el-checkbox v-for="k in KINDS" :key="k.v" :value="k.v">{{ k.t }}</el-checkbox>
        </el-checkbox-group>
      </div>
      <div class="kv">
        <span>上传成功后删除本地文件<small class="mute block">开启后本地只留记录（文件进回收站）；关闭则本地保留一份</small></span>
        <el-switch v-model="up.deleteAfter" />
      </div>
      <div><el-button size="small" :loading="testing" @click="test">测试连接</el-button></div>
    </template>
  </section>
</template>

<style scoped>
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
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.field label {
  font-size: 12px;
  color: var(--cc-mute);
}
.kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.block {
  display: block;
}
.row {
  display: flex;
  gap: 8px;
}
.ok {
  color: var(--cc-ok);
  margin-left: 6px;
}
</style>
