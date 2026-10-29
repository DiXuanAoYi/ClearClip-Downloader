<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { NotifyChannel, NotifySettings } from '../types'

const notify = defineModel<NotifySettings>({ required: true })
const secrets = reactive<Record<string, string>>({})
const has = reactive<Record<string, boolean>>({})
const testing = ref('')

const KINDS: { v: NotifyChannel['kind']; name: string; secret: string; target?: string; hint: string }[] = [
  { v: 'telegram', name: 'Telegram', secret: '机器人令牌（BotFather 给的）', target: 'chat id（数字）', hint: '先给机器人发一条消息，再用 getUpdates 查到 chat id。国内网络需要在“网络”里给 api.telegram.org 配代理。' },
  { v: 'bark', name: 'Bark（iPhone）', secret: '设备密钥', target: '服务器地址（留空用 https://api.day.app）', hint: '在 Bark App 里复制设备密钥。' },
  { v: 'serverchan', name: 'Server酱', secret: 'SendKey', hint: '在 sct.ftqq.com 登录后获取。' },
  { v: 'wecom', name: '企业微信群机器人', secret: 'Webhook 地址', hint: '群设置 → 添加群机器人，复制 Webhook 地址。' },
  { v: 'dingtalk', name: '钉钉群机器人', secret: 'Webhook 地址', hint: '安全设置选“自定义关键词”并填入“清影”或“下载”。' },
  { v: 'feishu', name: '飞书群机器人', secret: 'Webhook 地址', hint: '群设置 → 机器人 → 自定义机器人。' },
  { v: 'ntfy', name: 'ntfy', secret: '主题地址', hint: '例如 https://ntfy.sh/你的主题名，手机装 ntfy App 订阅同一个主题。' },
  { v: 'webhook', name: '自定义 Webhook', secret: 'Webhook 地址', hint: '收到 POST 的 JSON：{ app, title, body, text, time }，可以接 n8n、Home Assistant、IFTTT 等。' },
]
const info = (k: string) => KINDS.find((x) => x.v === k)!

async function refresh() {
  for (const c of notify.value.channels) has[c.id] = await api.secretHas(`notify.${c.id}`).catch(() => false)
}
onMounted(refresh)

function add(kind: NotifyChannel['kind']) {
  const id = Math.random().toString(36).slice(2, 10)
  notify.value.channels.push({ id, name: info(kind).name, enabled: true, kind, target: '' })
  has[id] = false
}

async function saveSecret(c: NotifyChannel) {
  try {
    await api.secretSet(`notify.${c.id}`, secrets[c.id] ?? '')
    secrets[c.id] = ''
    has[c.id] = await api.secretHas(`notify.${c.id}`)
    ElMessage.success('已加密保存')
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

async function remove(i: number) {
  const c = notify.value.channels[i]
  await api.secretSet(`notify.${c.id}`, '').catch(() => {})
  notify.value.channels.splice(i, 1)
}

async function test(c: NotifyChannel) {
  testing.value = c.id
  try {
    // 先让最新设置落盘（设置自动保存有 0.4 秒延迟）
    await new Promise((r) => setTimeout(r, 700))
    await api.notifyTest(c.id)
    ElMessage.success('测试通知已发送，请到手机上查看')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    testing.value = ''
  }
}
</script>

<template>
  <section class="group card">
    <h3>推送通知</h3>
    <small class="mute">把下载完成、失败、开播、订阅更新、登录失效推送到手机或聊天工具。令牌和 Webhook 地址加密保存在本机，不会写进设置文件，也不会进入备份。</small>
    <div class="events">
      <el-checkbox v-model="notify.onDone" size="small">下载完成</el-checkbox>
      <el-checkbox v-model="notify.onFailed" size="small">下载失败</el-checkbox>
      <el-checkbox v-model="notify.onLive" size="small">直播</el-checkbox>
      <el-checkbox v-model="notify.onSub" size="small">订阅更新</el-checkbox>
      <el-checkbox v-model="notify.onAccount" size="small">登录失效</el-checkbox>
    </div>
    <div v-for="(c, i) in notify.channels" :key="c.id" class="ch">
      <div class="top">
        <el-switch v-model="c.enabled" size="small" />
        <el-input v-model="c.name" size="small" class="name" />
        <el-tag size="small" effect="plain">{{ info(c.kind).name }}</el-tag>
        <el-button link size="small" :loading="testing === c.id" @click="test(c)">发送测试</el-button>
        <el-button link size="small" @click="remove(i)">删除</el-button>
      </div>
      <div class="row">
        <el-input v-model="secrets[c.id]" size="small" type="password" show-password :placeholder="has[c.id] ? `已保存（输入新的可替换）` : info(c.kind).secret" />
        <el-button size="small" :disabled="!(secrets[c.id] ?? '').trim()" @click="saveSecret(c)">保存</el-button>
      </div>
      <el-input v-if="info(c.kind).target" v-model="c.target" size="small" :placeholder="info(c.kind).target" />
      <small class="mute">{{ info(c.kind).hint }}</small>
    </div>
    <el-dropdown trigger="click" @command="(k: NotifyChannel['kind']) => add(k)">
      <el-button size="small">添加渠道 ▾</el-button>
      <template #dropdown>
        <el-dropdown-menu>
          <el-dropdown-item v-for="k in KINDS" :key="k.v" :command="k.v">{{ k.name }}</el-dropdown-item>
        </el-dropdown-menu>
      </template>
    </el-dropdown>
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
.events {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
}
.ch {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.top {
  display: flex;
  gap: 10px;
  align-items: center;
}
.name {
  flex: 1;
}
.row {
  display: flex;
  gap: 8px;
}
</style>
