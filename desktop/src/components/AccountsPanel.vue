<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { ElMessage, ElMessageBox } from 'element-plus'
import { open } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { AccountSummary } from '../types'
import { formatDateTime, formatRelative } from '../utils/format'
import { METHOD_NAME, SITE_GUIDES, guideFor, type LoginMethod } from '../utils/siteGuides'

const app = useAppStore()
const accounts = ref<AccountSummary[]>([])
const site = ref('bilibili')
const customDomain = ref('')
const label = ref('')
const loginOpen = ref(false)
const pasteVisible = ref(false)
const pasteText = ref('')
const busy = ref(false)

const providers = computed(() => app.info?.providers ?? [])
/** 内置平台以外、有登录说明的常用网站 */
const extraSites = computed(() => SITE_GUIDES.filter((g) => !providers.value.some((p) => p.id === g.site)))
const targetSite = computed(() => (site.value === 'other' ? normalizeDomain(customDomain.value) : site.value))
const guide = computed(() => (targetSite.value ? guideFor(targetSite.value) : null))
/** 谷歌系网站会拦截内嵌浏览器登录，只能用扩展或导入 Cookie。 */
const loginBlocked = computed(() => !!guide.value?.embeddedBlocked)
const recommended = computed<LoginMethod | null>(() => guide.value?.methods[0] ?? null)
const methodsEl = ref<HTMLElement | null>(null)
const methodsText = computed(() => (guide.value?.methods ?? []).map((m) => METHOD_NAME[m]).join(' → '))

/** 选择网站：已知网站直接选中，其他填到“其他网站” */
function selectSite(s: string) {
  const known = providers.value.some((p) => p.id === s) || extraSites.value.some((g) => g.site === s)
  if (known) site.value = s
  else {
    site.value = 'other'
    customDomain.value = s
  }
}

// 从出错提示跳过来：选中网站，能用内置登录时直接打开登录窗口
watch(
  () => app.loginRequest,
  async (req) => {
    if (!req) return
    app.loginRequest = null
    selectSite(req)
    await nextTick()
    methodsEl.value?.scrollIntoView({ behavior: 'smooth', block: 'center' })
    if (!loginBlocked.value && recommended.value === 'embedded') await openLogin()
  },
  { immediate: true },
)
const now = () => Math.floor(Date.now() / 1000)

function normalizeDomain(v: string) {
  return v
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/^www\./, '')
}

function expiryText(a: AccountSummary) {
  if (!a.expiresAt) return '会话 Cookie'
  const left = a.expiresAt - now()
  if (left <= 0) return '已过期'
  if (left < 3 * 86400) return `${Math.ceil(left / 3600)} 小时后过期`
  return `${formatDateTime(a.expiresAt)} 过期`
}

function expiryClass(a: AccountSummary) {
  if (!a.expiresAt) return ''
  const left = a.expiresAt - now()
  return left <= 0 ? 'bad' : left < 3 * 86400 ? 'warn' : ''
}

async function load() {
  accounts.value = await api.listAccounts()
}
let unlistenAccounts: UnlistenFn | undefined
let unlistenLogin: UnlistenFn | undefined
onMounted(async () => {
  await load()
  // 浏览器扩展同步 Cookie、定期检查登录状态后刷新
  unlistenAccounts = await listen('accounts://updated', load)
  // 登录窗口自动识别到登录完成
  unlistenLogin = await listen<string>('login://auto-saved', async (e) => {
    if (e.payload === targetSite.value) loginOpen.value = false
    await load()
    ElMessage.success('已识别到登录，登录状态已自动保存')
  })
})
onUnmounted(() => {
  unlistenAccounts?.()
  unlistenLogin?.()
})

async function run(fn: () => Promise<AccountSummary[] | void>, ok?: string) {
  busy.value = true
  try {
    const r = await fn()
    if (Array.isArray(r)) accounts.value = r
    if (ok) ElMessage.success(ok)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

function requireSite(): string | null {
  if (!targetSite.value) {
    ElMessage.warning('请先选择网站，或填写网站域名（例如 youtube.com）。')
    return null
  }
  return targetSite.value
}

async function openLogin() {
  const s = requireSite()
  if (!s) return
  await run(async () => {
    await api.openLogin(s, site.value === 'other' ? `https://www.${s}/` : undefined, label.value.trim() || undefined)
    loginOpen.value = true
  })
}

async function saveLogin() {
  const s = requireSite()
  if (!s) return
  await run(() => api.saveLoginCookies(s, label.value.trim() || undefined), '登录状态已保存')
  loginOpen.value = false
}

async function importFile() {
  const path = await open({ title: '选择 cookies.txt', filters: [{ name: 'cookies.txt', extensions: ['txt'] }] })
  if (typeof path !== 'string') return
  await run(() => api.importCookiesFile(path, label.value.trim() || undefined), 'Cookie 已导入')
}

async function importPaste() {
  const text = pasteText.value.trim()
  if (!text) return
  const s = text.includes('\t') ? '' : requireSite()
  if (s === null) return
  await run(() => api.importCookiesText(s, text, label.value.trim() || undefined), 'Cookie 已保存')
  pasteVisible.value = false
  pasteText.value = ''
}

const checking = ref<string | null>(null)

function validText(a: AccountSummary) {
  if (a.valid === null || !a.checkedAt) return ''
  return a.valid ? `登录有效（${formatRelative(a.checkedAt)}检查）` : `登录已失效（${formatRelative(a.checkedAt)}检查）`
}

function relogin(a: AccountSummary) {
  label.value = a.label === '默认' ? '' : a.label
  selectSite(a.site)
  nextTick(() => methodsEl.value?.scrollIntoView({ behavior: 'smooth', block: 'center' }))
  if (!guideFor(a.site).embeddedBlocked) openLogin()
}

async function check(a: AccountSummary) {
  checking.value = a.id
  try {
    const st = await api.checkAccount(a.id)
    if (st.loggedIn) ElMessage.success(`登录有效：${st.userName ?? ''}${st.vip ? `（${st.vip}）` : ''}`)
    else ElMessage.warning(`${a.siteName}账号“${a.label}”未登录或已失效，请重新登录。`)
    await load()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    checking.value = null
  }
}

async function makeDefault(a: AccountSummary) {
  await run(() => api.setDefaultAccount(a.id), '已设为默认')
}

async function rename(a: AccountSummary) {
  try {
    const { value } = await ElMessageBox.prompt('账号名称', '重命名', { inputValue: a.label, confirmButtonText: '保存', cancelButtonText: '取消' })
    if (value?.trim()) await run(() => api.renameAccount(a.id, value.trim()))
  } catch {
    /* 取消 */
  }
}

async function remove(a: AccountSummary) {
  try {
    await ElMessageBox.confirm(`删除 ${a.siteName} 的账号“${a.label}”？只删除清影里保存的 Cookie，不影响网站上的账号。`, '删除账号', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }
  await run(() => api.deleteAccount(a.id), '已删除')
}
</script>

<template>
  <div class="panel">
    <el-alert v-if="app.info && !app.info.keyInKeyring" type="warning" show-icon :closable="false" title="系统钥匙串不可用，Cookie 的加密密钥保存在本机的应用数据目录中。" />

    <section class="card block">
      <h3>已保存的账号</h3>
      <div v-if="!accounts.length" class="mute empty">还没有保存任何账号。需要登录才能下载的内容，请在下方添加。</div>
      <div v-for="a in accounts" :key="a.id" class="acc">
        <div class="info">
          <div>
            <b>{{ a.siteName }}</b>
            <span class="label">{{ a.label }}</span>
            <span v-if="a.isDefault" class="chip acc-chip">默认</span>
            <span v-if="a.userName" class="mute"> · {{ a.userName }}</span>
          </div>
          <small class="mute">
            {{ a.cookieCount }} 个 Cookie · 更新于 {{ formatDateTime(a.updatedAt) }} ·
            <span :class="expiryClass(a)">{{ expiryText(a) }}</span>
            <template v-if="validText(a)"> · <span :class="a.valid ? 'ok' : 'bad'">{{ validText(a) }}</span></template>
          </small>
        </div>
        <div class="ops">
          <el-button v-if="a.valid === false || expiryClass(a) === 'bad'" link size="small" type="primary" @click="relogin(a)">重新登录</el-button>
          <el-button v-if="a.checkable" link size="small" :loading="checking === a.id" @click="check(a)">检测</el-button>
          <el-button v-if="!a.isDefault" link size="small" type="primary" @click="makeDefault(a)">设为默认</el-button>
          <el-button link size="small" @click="rename(a)">重命名</el-button>
          <el-button link size="small" @click="remove(a)">删除</el-button>
        </div>
      </div>
    </section>

    <section class="card block">
      <h3>添加账号</h3>
      <div class="row">
        <el-select v-model="site" size="small" class="site">
          <el-option v-for="p in providers" :key="p.id" :value="p.id" :label="p.name" />
          <el-option v-for="g in extraSites" :key="g.site" :value="g.site" :label="g.name" />
          <el-option value="other" label="其他网站…" />
        </el-select>
        <el-input v-if="site === 'other'" v-model="customDomain" size="small" placeholder="网站域名，例如 x.com" class="domain" />
        <el-input v-model="label" size="small" placeholder="账号名称（可选），例如“大会员”" class="label-input" />
      </div>

      <div v-if="guide" class="guide">
        <div><span class="mute">不登录：</span>{{ guide.anonymous }}</div>
        <div><span class="mute">需要登录：</span>{{ guide.needLogin }}</div>
        <div>
          <span class="mute">推荐方式：</span>{{ methodsText }}<template v-if="guide.loginTip">。{{ guide.loginTip }}</template>
        </div>
        <a class="link" @click="api.openUrl(`https://github.com/${app.info?.repo ?? 'DiXuanAoYi/ClearClip-Downloader'}/blob/main/docs/login-guide.md`)">各网站登录说明</a>
      </div>

      <el-alert v-if="loginBlocked" type="info" :closable="false" show-icon>
        <template #title>{{ guide?.name }}会拦截内置登录窗口，请用浏览器扩展同步或导入 cookies.txt</template>
        建议在浏览器的无痕 / 隐私窗口里登录，同步或导出 Cookie 后关闭该窗口，不要再在浏览器里使用这次登录，否则 Cookie 很快会失效。
      </el-alert>

      <div ref="methodsEl" class="methods">
        <div class="method" :class="{ rec: recommended === 'embedded' }">
          <b>内置登录窗口<span v-if="recommended === 'embedded'" class="chip rec-chip">推荐</span></b>
          <small class="mute">
            在弹出的官网窗口里登录。{{ guide?.autoDetect ? '登录完成后会自动保存并关闭窗口，也可以手动点“保存登录状态”。' : '完成后点“保存登录状态”。' }}
          </small>
          <div class="row">
            <el-button v-if="!loginOpen" size="small" :disabled="loginBlocked" :loading="busy" @click="openLogin">打开登录窗口</el-button>
            <el-button v-else size="small" type="primary" :loading="busy" @click="saveLogin">保存登录状态</el-button>
          </div>
        </div>
        <div class="method" :class="{ rec: recommended === 'cookies' }">
          <b>导入 cookies.txt<span v-if="recommended === 'cookies'" class="chip rec-chip">推荐</span></b>
          <small class="mute">Netscape 格式，可用浏览器扩展导出；会按网站自动分组保存。</small>
          <div class="row"><el-button size="small" :loading="busy" @click="importFile">选择文件…</el-button></div>
        </div>
        <div class="method">
          <b>粘贴 Cookie</b>
          <small class="mute">粘贴 cookies.txt 内容，或请求头里的 Cookie 字符串。</small>
          <div class="row"><el-button size="small" @click="pasteVisible = true">粘贴…</el-button></div>
        </div>
        <div class="method" :class="{ rec: recommended === 'extension' }">
          <b>浏览器扩展同步<span v-if="recommended === 'extension'" class="chip rec-chip">推荐</span></b>
          <small class="mute">安装“发送到清影”扩展，在已登录的网站上点“同步此网站的登录 Cookie”，适合 YouTube 等拦截内置登录窗口的网站。</small>
          <div class="row"><el-button size="small" @click="app.goSettings('phone')">设置扩展</el-button></div>
        </div>
      </div>
      <small class="mute">Cookie 加密保存在本机，只会发送给它所属的网站。使用登录账号大量下载有被平台限制的风险。</small>
    </section>

    <el-dialog v-model="pasteVisible" title="粘贴 Cookie" width="560px">
      <p class="mute small">当前网站：{{ targetSite || '未选择' }}。粘贴 cookies.txt 内容时会按文件里的域名自动分组。</p>
      <el-input v-model="pasteText" type="textarea" :rows="8" class="mono" placeholder="SESSDATA=...; bili_jct=...  或  cookies.txt 内容" />
      <template #footer>
        <el-button @click="pasteVisible = false">取消</el-button>
        <el-button type="primary" :loading="busy" @click="importPaste">保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.block {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
h3 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-mute);
  font-weight: 500;
  letter-spacing: 0.08em;
}
.empty {
  padding: 8px 0;
}
.acc {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  border-top: 1px dashed var(--cc-line);
}
.acc:first-of-type {
  border-top: 0;
}
.info {
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.label {
  margin-left: 8px;
}
.acc-chip {
  margin-left: 6px;
  border-color: var(--cc-acc);
  color: var(--cc-acc);
}
.ops {
  display: flex;
  gap: 4px;
  flex: none;
}
.ops :deep(.el-button) {
  margin-left: 6px;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.site {
  width: 150px;
}
.domain {
  width: 180px;
}
.label-input {
  width: 220px;
}
.methods {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 10px;
}
.method {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.method small {
  font-size: 11.5px;
}
.warn {
  color: #d9a441;
}
.ok {
  color: var(--cc-ok);
}
.guide {
  font-size: 12px;
  line-height: 1.7;
  background: var(--cc-side);
  border-radius: 8px;
  padding: 8px 12px;
}
.link {
  color: var(--cc-acc);
  cursor: pointer;
  font-size: 12px;
}
.method.rec {
  border-color: var(--cc-acc);
}
.rec-chip {
  margin-left: 6px;
  border-color: var(--cc-acc);
  color: var(--cc-acc);
  font-weight: 400;
}
.bad {
  color: var(--cc-err);
}
.small {
  font-size: 12px;
  margin: 0 0 8px;
}
</style>
