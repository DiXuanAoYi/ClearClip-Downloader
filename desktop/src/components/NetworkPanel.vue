<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { NetworkSettings, Route, RouteSpeed, RouteTest } from '../types'

const net = defineModel<NetworkSettings>({ required: true })

const PROXY_HINT = 'http://127.0.0.1:7890 或 socks5://127.0.0.1:1080'
const filter = ref('')
const newPattern = ref('')
const newRoute = ref('direct')
const testUrl = ref('https://www.youtube.com/')
const testing = ref(false)
const testResult = ref<{ ok: boolean; text: string } | null>(null)

/** 下拉框用字符串表示出口：direct / system / proxy:<id> */
function routeKey(r: Route): string {
  return r.kind === 'proxy' ? `proxy:${r.id}` : r.kind
}
function routeFromKey(k: string): Route {
  if (k.startsWith('proxy:')) return { kind: 'proxy', id: k.slice(6) }
  return k === 'direct' ? { kind: 'direct' } : { kind: 'system' }
}
function routeName(r: Route): string {
  if (r.kind === 'direct') return '直连'
  if (r.kind === 'system') return '系统代理'
  return net.value.proxies.find((p) => p.id === r.id)?.name ?? `代理（已删除）`
}

const routeOptions = computed(() => [
  { value: 'direct', label: '直连' },
  { value: 'system', label: '系统代理' },
  ...net.value.proxies.map((p) => ({ value: `proxy:${p.id}`, label: `代理：${p.name}` })),
])

const rules = computed(() => {
  const f = filter.value.trim().toLowerCase()
  return net.value.rules.map((r, i) => ({ r, i })).filter(({ r }) => !f || r.pattern.includes(f))
})

function setDefault(k: string) {
  net.value = { ...net.value, defaultRoute: routeFromKey(k) }
}

function setRule(i: number, k: string) {
  const rules = [...net.value.rules]
  rules[i] = { ...rules[i], route: routeFromKey(k) }
  net.value = { ...net.value, rules }
}

function removeRule(i: number) {
  const rules = [...net.value.rules]
  rules.splice(i, 1)
  net.value = { ...net.value, rules }
}

function addRule() {
  const p = newPattern.value
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/^\*?\./, '')
  if (!p || !p.includes('.')) {
    ElMessage.warning('请填写域名，例如 youtube.com')
    return
  }
  const rules = net.value.rules.filter((r) => r.pattern !== p)
  rules.unshift({ pattern: p, route: routeFromKey(newRoute.value) })
  net.value = { ...net.value, rules }
  newPattern.value = ''
}

function addProxy() {
  const id = `p${Date.now().toString(36)}`
  net.value = { ...net.value, proxies: [...net.value.proxies, { id, name: `代理 ${net.value.proxies.length + 1}`, url: 'http://127.0.0.1:7890' }] }
}

function updateProxy(i: number, field: 'name' | 'url', v: string) {
  const proxies = [...net.value.proxies]
  proxies[i] = { ...proxies[i], [field]: v.trim() }
  net.value = { ...net.value, proxies }
}

function removeProxy(i: number) {
  const id = net.value.proxies[i].id
  const used = net.value.rules.some((r) => r.route.kind === 'proxy' && r.route.id === id) || (net.value.defaultRoute.kind === 'proxy' && net.value.defaultRoute.id === id)
  if (used) {
    ElMessage.warning('还有规则在使用这个代理，请先修改这些规则。')
    return
  }
  const proxies = [...net.value.proxies]
  proxies.splice(i, 1)
  net.value = { ...net.value, proxies }
}

const speedUrl = ref('')
const speeding = ref(false)
const speeds = ref<RouteSpeed[]>([])

async function runSpeed() {
  const url = speedUrl.value.trim()
  if (!url) return ElMessage.warning('请填写要测速的地址（最好是一个视频文件，或这个网站上较大的资源）。')
  speeding.value = true
  speeds.value = []
  try {
    speeds.value = await api.routeSpeedtest(url)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    speeding.value = false
  }
}

const fastest = computed(() => [...speeds.value].filter((s) => s.ok).sort((a, b) => b.kbps - a.kbps)[0])

/** 把测速最快的线路设成这个网站的出口规则。 */
function useRoute(r: RouteSpeed) {
  let host = ''
  try {
    host = new URL(speedUrl.value.trim()).hostname
  } catch {
    return
  }
  const pattern = host.split('.').slice(-2).join('.')
  const rules = net.value.rules.filter((x) => x.pattern !== pattern)
  rules.push({ pattern, route: r.route })
  net.value = { ...net.value, rules }
  ElMessage.success(`已把 ${pattern} 的出口设为“${r.name}”`)
}

async function runTest() {
  testing.value = true
  testResult.value = null
  try {
    const r: RouteTest = await api.testRoute(testUrl.value.trim())
    testResult.value = { ok: true, text: `通过${routeName(r.route)}访问成功：HTTP ${r.status}，耗时 ${r.millis} 毫秒` }
  } catch (e) {
    testResult.value = { ok: false, text: errorText(e) }
  } finally {
    testing.value = false
  }
}
</script>

<template>
  <div class="panel">
    <section class="card block">
      <h3>线路测速</h3>
      <p class="mute small">对直连、系统代理和每个自定义代理，各下载这个地址开头的一小段（最多 1.5 MB / 6 秒），比较首字节耗时和速度。找出访问某个网站最快的出口后，可以一键设为这个网站的出口规则。</p>
      <div class="row">
        <el-input v-model="speedUrl" size="small" class="grow mono" placeholder="https://example.com/video.mp4" @keyup.enter="runSpeed" />
        <el-button size="small" :loading="speeding" @click="runSpeed">开始测速</el-button>
      </div>
      <div v-for="r in speeds" :key="r.name" class="speedrow">
        <span class="sname">{{ r.name }}<el-tag v-if="fastest && fastest.name === r.name" size="small" type="success" effect="plain">最快</el-tag></span>
        <span v-if="r.ok" class="mono">{{ r.kbps >= 1024 ? (r.kbps / 1024).toFixed(1) + ' MB/s' : r.kbps + ' KB/s' }} · 首字节 {{ r.ttfbMs }} ms</span>
        <span v-else class="bad">{{ r.error }}</span>
        <el-button v-if="r.ok" size="small" link type="primary" @click="useRoute(r)">设为此网站的出口</el-button>
      </div>
    </section>

    <section class="card block">
      <h3>默认出口与测试</h3>
      <p class="mute small">国内平台（抖音、B站等）走海外代理时常被拦截，默认直连；YouTube、Pornhub、Pixiv 等默认走系统代理。没有匹配规则的网站使用默认出口。</p>
      <div class="row">
        <span>默认出口</span>
        <el-select :model-value="routeKey(net.defaultRoute)" size="small" class="route" @update:model-value="setDefault">
          <el-option v-for="o in routeOptions" :key="o.value" :value="o.value" :label="o.label" />
        </el-select>
      </div>
      <div class="row">
        <el-input v-model="testUrl" size="small" class="grow mono" placeholder="https://www.youtube.com/" />
        <el-button size="small" :loading="testing" @click="runTest">测试连通</el-button>
      </div>
      <small v-if="testResult" :class="testResult.ok ? 'ok' : 'bad'">{{ testResult.text }}</small>
    </section>

    <section class="card block">
      <h3>自定义代理</h3>
      <div v-if="!net.proxies.length" class="mute small">没有自定义代理。“系统代理”会读取系统设置和 HTTP_PROXY 等环境变量。</div>
      <div v-for="(p, i) in net.proxies" :key="p.id" class="row">
        <el-input :model-value="p.name" size="small" class="pname" @change="updateProxy(i, 'name', $event)" />
        <el-input :model-value="p.url" size="small" class="grow mono" :placeholder="PROXY_HINT" @change="updateProxy(i, 'url', $event)" />
        <el-button size="small" link @click="removeProxy(i)">删除</el-button>
      </div>
      <div><el-button size="small" @click="addProxy">添加代理</el-button></div>
    </section>

    <section class="card block">
      <h3>按网站分流规则（{{ net.rules.length }} 条）</h3>
      <div class="row">
        <el-input v-model="newPattern" size="small" class="grow mono" placeholder="域名，例如 example.com（同时匹配子域名）" @keyup.enter="addRule" />
        <el-select v-model="newRoute" size="small" class="route">
          <el-option v-for="o in routeOptions" :key="o.value" :value="o.value" :label="o.label" />
        </el-select>
        <el-button size="small" type="primary" @click="addRule">添加</el-button>
      </div>
      <el-input v-model="filter" size="small" clearable placeholder="筛选规则" />
      <div class="rules">
        <div v-for="{ r, i } in rules" :key="r.pattern" class="rule">
          <span class="mono ellipsis">{{ r.pattern }}</span>
          <el-select :model-value="routeKey(r.route)" size="small" class="route" @update:model-value="setRule(i, $event)">
            <el-option v-for="o in routeOptions" :key="o.value" :value="o.value" :label="o.label" />
          </el-select>
          <el-button size="small" link @click="removeRule(i)">删除</el-button>
        </div>
      </div>
    </section>
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
.speedrow {
  display: grid;
  grid-template-columns: 150px 1fr auto;
  gap: 10px;
  align-items: center;
  font-size: 12.5px;
  padding: 3px 0;
}
.sname {
  display: flex;
  gap: 6px;
  align-items: center;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.row :deep(.el-button) {
  margin-left: 0;
}
.grow {
  flex: 1;
  min-width: 0;
}
.route {
  width: 160px;
}
.pname {
  width: 140px;
}
.rules {
  max-height: 320px;
  overflow: auto;
  display: flex;
  flex-direction: column;
}
.rule {
  display: grid;
  grid-template-columns: 1fr 160px auto;
  gap: 8px;
  align-items: center;
  padding: 4px 0;
  border-top: 1px dashed var(--cc-line);
}
.small {
  font-size: 12px;
  margin: 0;
  line-height: 1.6;
}
.ok {
  color: var(--cc-ok);
}
.bad {
  color: var(--cc-err);
}
</style>
