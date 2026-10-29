<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { api, errorText, events } from '../api'
import type { SafeEntry, SafeStatus } from '../types'
import { formatBytes, formatDateTime } from '../utils/format'
import JobList from '../components/toolbox/JobList.vue'

const status = ref<SafeStatus | null>(null)
const entries = ref<SafeEntry[]>([])
const selected = ref<SafeEntry[]>([])
const password = ref('')
const confirm = ref('')
const busy = ref(false)
const error = ref('')
const deleteOriginal = ref(true)
const changing = ref(false)
const oldPw = ref('')
const newPw = ref('')
const KIND: Record<string, string> = { video: '视频', audio: '音频', image: '图片', other: '文件' }
let un: UnlistenFn[] = []

function onSelect(v: SafeEntry[]) {
  selected.value = v
}

const mismatch = computed(() => !!confirm.value && password.value !== confirm.value)

async function refresh() {
  status.value = await api.safeboxStatus()
  entries.value = status.value.unlocked ? await api.safeboxList().catch(() => []) : []
}

async function run(fn: () => Promise<unknown>, ok?: string) {
  busy.value = true
  error.value = ''
  try {
    await fn()
    if (ok) ElMessage.success(ok)
  } catch (e) {
    error.value = errorText(e)
    ElMessage.error(error.value)
  } finally {
    busy.value = false
    await refresh().catch(() => {})
  }
}

const create = () =>
  run(async () => {
    if (password.value.length < 6) throw new Error('密码至少 6 个字符。')
    if (password.value !== confirm.value) throw new Error('两次输入的密码不一致。')
    await api.safeboxCreate(password.value)
    password.value = confirm.value = ''
  }, '保险箱已创建')

const unlock = () =>
  run(async () => {
    await api.safeboxUnlock(password.value)
    password.value = ''
  })

const lock = () => run(() => api.safeboxLock())

async function addFiles() {
  const picked = await open({ multiple: true, title: '选择要放进保险箱的文件' })
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : []
  if (!paths.length) return
  if (deleteOriginal.value) {
    try {
      await ElMessageBox.confirm(`加密完成后会覆盖并删除这 ${paths.length} 个原文件。原文件删除后只能从保险箱里取回。继续？`, '加入保险箱', {
        type: 'warning',
        confirmButtonText: '加密并删除原文件',
        cancelButtonText: '取消',
      })
    } catch {
      return
    }
  }
  await run(() => api.safeboxAddFiles(paths, deleteOriginal.value), `已开始加密 ${paths.length} 个文件`)
}

const openEntry = (e: SafeEntry) => run(() => api.safeboxOpen(e.id))

async function exportEntry(e: SafeEntry) {
  const dir = await open({ directory: true, title: '解密导出到哪个文件夹' })
  if (typeof dir !== 'string') return
  await run(() => api.safeboxExport(e.id, dir), '已开始解密导出')
}

async function remove(list: SafeEntry[]) {
  if (!list.length) return
  try {
    await ElMessageBox.confirm(`从保险箱永久删除 ${list.length} 个文件？无法恢复。`, '删除', { type: 'warning', confirmButtonText: '永久删除', cancelButtonText: '取消' })
  } catch {
    return
  }
  await run(() => api.safeboxRemove(list.map((e) => e.id)), '已删除')
}

const changePw = () =>
  run(async () => {
    await api.safeboxChangePassword(oldPw.value, newPw.value)
    changing.value = false
    oldPw.value = newPw.value = ''
  }, '密码已修改')

onMounted(async () => {
  await refresh().catch(() => {})
  un.push(await events.onSafebox(() => refresh().catch(() => {})))
})
onBeforeUnmount(() => un.forEach((f) => f()))
</script>

<template>
  <div class="page">
    <div class="head">
      <h2>加密保险箱</h2>
      <span v-if="status?.unlocked" class="mute">{{ status.count }} 个文件 · {{ formatBytes(status.totalBytes) }}</span>
      <div class="sp" />
      <template v-if="status?.unlocked">
        <el-button size="small" @click="changing = true">修改密码</el-button>
        <el-button size="small" @click="lock">锁定保险箱</el-button>
      </template>
    </div>

    <!-- 还没有保险箱 -->
    <section v-if="status && !status.exists" class="card box">
      <h3>创建保险箱</h3>
      <p class="mute">
        放进保险箱的文件会用 AES-256 加密，文件名也看不出来；只有输入密码才能打开。它和“应用锁”是两回事：应用锁只是遮住界面，保险箱才真正加密文件。
      </p>
      <p class="warn"><b>忘记密码无法找回</b>，文件也就打不开了。请把密码记在安全的地方。</p>
      <el-input v-model="password" type="password" show-password placeholder="密码（至少 6 个字符）" autocomplete="off" />
      <el-input v-model="confirm" type="password" show-password placeholder="再输入一次" autocomplete="off" @keyup.enter="create" />
      <div v-if="mismatch" class="err">两次输入的密码不一致。</div>
      <div><el-button type="primary" :loading="busy" :disabled="password.length < 6 || mismatch || !confirm" @click="create">创建</el-button></div>
    </section>

    <!-- 已锁定 -->
    <section v-else-if="status && !status.unlocked" class="card box">
      <h3>保险箱已锁定</h3>
      <el-input v-model="password" type="password" show-password placeholder="保险箱密码" autocomplete="off" :disabled="status.lockedOutSecs > 0" @keyup.enter="unlock" />
      <div v-if="status.lockedOutSecs > 0" class="err">输错次数太多，请 {{ status.lockedOutSecs }} 秒后再试。</div>
      <div v-if="error" class="err selectable">{{ error }}</div>
      <div><el-button type="primary" :loading="busy" :disabled="!password || status.lockedOutSecs > 0" @click="unlock">解锁</el-button></div>
    </section>

    <!-- 已解锁 -->
    <template v-else-if="status">
      <div class="bar">
        <el-button type="primary" size="small" @click="addFiles">添加文件…</el-button>
        <el-checkbox v-model="deleteOriginal" size="small">加密后覆盖并删除原文件</el-checkbox>
        <span class="mute small">媒体库里的作品可在“媒体库”里选中后“移入保险箱”。</span>
        <div class="sp" />
        <el-button v-if="selected.length" size="small" type="danger" plain @click="remove(selected)">删除所选（{{ selected.length }}）</el-button>
      </div>
      <el-table :data="entries" size="small" empty-text="保险箱是空的。点“添加文件…”放进第一个文件。" @selection-change="onSelect">
        <el-table-column type="selection" width="38" />
        <el-table-column label="名称" min-width="260">
          <template #default="{ row }">
            <span class="ellipsis" :title="row.name">{{ row.name }}</span>
          </template>
        </el-table-column>
        <el-table-column label="类型" width="70">
          <template #default="{ row }">{{ KIND[row.kind] }}</template>
        </el-table-column>
        <el-table-column label="大小" width="90">
          <template #default="{ row }">{{ formatBytes(row.size) }}</template>
        </el-table-column>
        <el-table-column label="加入时间" width="140">
          <template #default="{ row }">{{ formatDateTime(row.addedAt) }}</template>
        </el-table-column>
        <el-table-column label="操作" width="170">
          <template #default="{ row }">
            <el-button link size="small" type="primary" @click="openEntry(row)">打开</el-button>
            <el-button link size="small" @click="exportEntry(row)">解密导出</el-button>
            <el-button link size="small" @click="remove([row])">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
      <p class="mute small">
        “打开”会把文件解密到临时文件夹，再用系统默认程序打开；锁定保险箱、退出清影或下次启动时，这些临时文件会被覆盖删除。覆盖删除在固态硬盘上不能保证无法恢复，
        对安全要求高时请同时开启系统的整盘加密（BitLocker / FileVault）。
      </p>
      <JobList />
    </template>

    <el-dialog v-model="changing" title="修改保险箱密码" width="380px">
      <div class="col">
        <el-input v-model="oldPw" type="password" show-password placeholder="当前密码" autocomplete="off" />
        <el-input v-model="newPw" type="password" show-password placeholder="新密码（至少 6 个字符）" autocomplete="off" />
        <small class="mute">只重新加密保险箱的主密钥，文件不用重新加密，瞬间完成。</small>
      </div>
      <template #footer>
        <el-button @click="changing = false">取消</el-button>
        <el-button type="primary" :loading="busy" :disabled="!oldPw || newPw.length < 6" @click="changePw">修改</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.page {
  padding: 20px 24px;
  max-width: 1040px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}
h2 {
  margin: 0;
  font-size: 16px;
}
h3 {
  margin: 0;
  font-size: 14px;
}
.sp {
  flex: 1;
}
.box {
  padding: 18px 20px;
  max-width: 440px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.box p {
  margin: 0;
  font-size: 12.5px;
  line-height: 1.6;
}
.warn {
  color: var(--cc-err);
}
.err {
  color: var(--cc-err);
  font-size: 12.5px;
}
.bar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.small {
  font-size: 11.5px;
}
.col {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ellipsis {
  display: inline-block;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: bottom;
}
</style>
