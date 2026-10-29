<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const password = ref('')
const error = ref('')
const busy = ref(false)
const wait = ref(0)
const input = ref<{ focus: () => void } | null>(null)
let timer: number | undefined

function tick() {
  if (wait.value > 0) wait.value--
}

async function submit() {
  if (!password.value || busy.value || wait.value > 0) return
  busy.value = true
  error.value = ''
  try {
    await api.lockUnlock(password.value)
    password.value = ''
    await app.refreshLock()
  } catch (e) {
    error.value = errorText(e)
    password.value = ''
    await app.refreshLock()
    wait.value = app.lock.lockedOutSecs
    await nextTick()
    input.value?.focus()
  } finally {
    busy.value = false
  }
}

onMounted(() => {
  wait.value = app.lock.lockedOutSecs
  timer = window.setInterval(tick, 1000)
  input.value?.focus()
})
onBeforeUnmount(() => window.clearInterval(timer))
// 再次锁定后重新聚焦输入框
watch(
  () => app.lock.locked,
  async (v) => {
    if (v) {
      await nextTick()
      input.value?.focus()
    }
  },
)
</script>

<template>
  <div v-if="app.lock.locked" class="lock" role="dialog" aria-modal="true" aria-label="清影已锁定">
    <div class="box">
      <div class="brand">清<i>影</i></div>
      <p class="mute">已锁定，输入密码继续。下载任务仍在后台进行。</p>
      <el-input ref="input" v-model="password" type="password" show-password size="large" placeholder="密码" :disabled="wait > 0" autocomplete="off" @keyup.enter="submit" />
      <el-button type="primary" size="large" :loading="busy" :disabled="!password || wait > 0" @click="submit">{{ wait > 0 ? `请等待 ${wait} 秒` : '解锁' }}</el-button>
      <div v-if="error" class="err selectable">{{ error }}</div>
    </div>
  </div>
</template>

<style scoped>
.lock {
  position: fixed;
  inset: 0;
  z-index: 99999;
  background: var(--cc-bg);
  display: flex;
  align-items: center;
  justify-content: center;
}
.box {
  width: 300px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  text-align: center;
}
.brand {
  font-weight: 900;
  font-size: 30px;
  letter-spacing: 0.1em;
}
.brand i {
  font-style: normal;
  color: var(--cc-acc);
}
p {
  margin: 0;
  font-size: 12.5px;
}
.err {
  color: var(--cc-err);
  font-size: 12.5px;
}
</style>
