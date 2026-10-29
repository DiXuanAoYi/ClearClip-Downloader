<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import { useAppStore, type SettingsTab, type ViewName } from '../stores/app'

interface Cmd {
  id: string
  title: string
  group: string
  /** 额外的搜索词（拼音首字母、英文等） */
  keywords?: string
  run: () => void | Promise<void>
}

const app = useAppStore()
const open = ref(false)
const query = ref('')
const index = ref(0)
const input = ref<{ focus: () => void } | null>(null)

const VIEWS: [ViewName, string, string][] = [
  ['parse', '解析', 'parse jiexi'],
  ['queue', '下载队列', 'queue download xiazai duilie'],
  ['subs', '订阅', 'subscriptions dingyue'],
  ['live', '直播', 'live zhibo'],
  ['library', '媒体库', 'library meitiku'],
  ['tools', '工具箱', 'tools toolbox gongjuxiang'],
  ['safebox', '保险箱', 'safebox vault baoxianxiang'],
  ['settings', '设置', 'settings shezhi'],
]
const TABS: [SettingsTab, string][] = [
  ['download', '下载'],
  ['parse', '解析'],
  ['network', '网络'],
  ['accounts', '账号与 Cookie'],
  ['phone', '手机与浏览器扩展'],
  ['automation', '自动化'],
  ['ai', 'AI'],
  ['security', '安全与隐私'],
  ['components', '组件'],
  ['diagnostics', '诊断'],
  ['general', '通用'],
]

function parseText(q: string) {
  app.pendingParse = q
}

function patchSecurity(p: Record<string, unknown>) {
  if (!app.settings) return
  return app.patch({ security: { ...app.settings.security, ...p } })
}

const commands = computed<Cmd[]>(() => {
  const s = app.settings
  const list: Cmd[] = [
    ...VIEWS.map<Cmd>(([id, t, k]) => ({ id: `go-${id}`, title: `前往：${t}`, group: '页面', keywords: k, run: () => void (app.view = id) })),
    ...TABS.map<Cmd>(([id, t]) => ({ id: `tab-${id}`, title: `设置：${t}`, group: '设置', keywords: `settings ${id}`, run: () => app.goSettings(id) })),
    { id: 'pause', title: '暂停全部下载', group: '操作', keywords: 'pause zanting', run: () => api.pauseAll() },
    { id: 'resume', title: '继续全部下载', group: '操作', keywords: 'resume jixu', run: () => api.resumeAll() },
    { id: 'clear', title: '清除已完成的任务', group: '操作', keywords: 'clear qingchu', run: () => api.clearFinished() },
    { id: 'folder', title: '打开下载文件夹', group: '操作', keywords: 'folder open wenjianjia', run: () => (s ? api.revealFile(s.downloadDir) : undefined) },
    { id: 'lock', title: '立即锁定', group: '安全', keywords: 'lock suoding', run: async () => void ((await api.lockNow()) || ElMessage.info('还没有设置应用锁密码')) },
    {
      id: 'privacy',
      title: s?.security.privacyMode ? '关闭隐私模式' : '开启隐私模式',
      group: '安全',
      keywords: 'privacy yinsi',
      run: () => patchSecurity({ privacyMode: !s?.security.privacyMode }),
    },
    { id: 'metered', title: s?.meteredMode ? '关闭省流量模式' : '开启省流量模式', group: '网络', keywords: 'metered data saver shengliuliang', run: () => app.patch({ meteredMode: !s?.meteredMode }) },
    {
      id: 'float',
      title: s?.floatBall ? '关闭悬浮拖拽窗' : '开启悬浮拖拽窗',
      group: '桌面',
      keywords: 'float ball xuanfu',
      run: () => app.patch({ floatBall: !s?.floatBall }),
    },
    {
      id: 'theme',
      title: '切换主题（跟随系统 → 浅色 → 深色）',
      group: '桌面',
      keywords: 'theme dark light zhuti',
      run: () => app.patch({ theme: s?.theme === 'system' ? 'light' : s?.theme === 'light' ? 'dark' : 'system' }),
    },
    {
      id: 'update',
      title: '检查更新',
      group: '桌面',
      keywords: 'update gengxin',
      run: async () => {
        const u = await api.checkUpdate()
        ElMessage({ type: u.hasUpdate ? 'success' : 'info', message: u.hasUpdate ? `发现新版本 v${u.latest}` : '已经是最新版本' })
      },
    },
  ]
  const q = query.value.trim()
  if (q) {
    if (/https?:\/\//i.test(q)) list.unshift({ id: 'parse-q', title: `解析这个链接：${q.slice(0, 60)}`, group: '输入', run: () => parseText(q) })
    list.push({ id: 'search-q', title: `在媒体库搜索“${q}”`, group: '输入', run: () => app.goLibraryQuery(q) })
  }
  return list
})

/** 按子序列匹配：输入的字符按顺序出现在标题或关键词里即可。 */
function score(cmd: Cmd, q: string): number {
  if (!q) return 1
  const hay = `${cmd.title} ${cmd.keywords ?? ''}`.toLowerCase()
  const needle = q.toLowerCase().replace(/\s+/g, '')
  const at = hay.indexOf(q.toLowerCase())
  if (at >= 0) return 1000 - at
  let pos = 0
  for (const ch of needle) {
    pos = hay.indexOf(ch, pos)
    if (pos < 0) return 0
    pos++
  }
  return 100 - Math.min(pos, 90)
}

const results = computed(() => {
  const q = query.value.trim()
  return commands.value
    // 粘贴的链接排第一；“在媒体库搜索”只作为兜底放在最后
    .map((c) => ({ c, s: c.id === 'parse-q' ? 10_000 : c.id === 'search-q' ? 0.5 : score(c, q) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .map((x) => x.c)
    .slice(0, 12)
})

watch(query, () => (index.value = 0))

async function show() {
  if (app.lock.locked) return
  query.value = ''
  index.value = 0
  open.value = true
  await nextTick()
  input.value?.focus()
}

async function run(c: Cmd | undefined) {
  if (!c) return
  open.value = false
  try {
    await c.run()
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    if (open.value) open.value = false
    else void show()
  } else if (open.value && e.key === 'Escape') {
    open.value = false
  }
}
function onInputKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    index.value = Math.min(index.value + 1, results.value.length - 1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    index.value = Math.max(index.value - 1, 0)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    void run(results.value[index.value])
  }
}

onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
defineExpose({ show })
</script>

<template>
  <div v-if="open" class="mask" @mousedown.self="open = false">
    <div class="box card" role="dialog" aria-label="命令面板">
      <el-input ref="input" v-model="query" size="large" placeholder="输入要做的事，例如“隐私”“队列”“设置 网络”，或粘贴链接" @keydown="onInputKey" />
      <div class="list">
        <div v-for="(c, i) in results" :key="c.id" class="item" :class="{ on: i === index }" @mousemove="index = i" @click="run(c)">
          <span class="g">{{ c.group }}</span>
          <span class="t">{{ c.title }}</span>
        </div>
        <div v-if="!results.length" class="mute empty">没有匹配的命令</div>
      </div>
      <div class="foot mute">↑↓ 选择 · Enter 执行 · Esc 关闭 · Ctrl+K 再次打开</div>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 3500;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  justify-content: center;
  padding-top: 11vh;
}
.box {
  width: min(560px, 92vw);
  max-height: 70vh;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-self: flex-start;
}
.list {
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.item {
  display: flex;
  gap: 10px;
  align-items: baseline;
  padding: 8px 10px;
  border-radius: 7px;
  cursor: pointer;
  font-size: 13.5px;
}
.item.on {
  background: var(--cc-acc-soft);
}
.g {
  flex: 0 0 38px;
  font-size: 11px;
  color: var(--cc-mute);
}
.empty {
  padding: 14px;
  text-align: center;
}
.foot {
  font-size: 11px;
  padding: 2px 4px 0;
}
</style>
