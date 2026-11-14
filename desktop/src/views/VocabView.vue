<script setup lang="ts">
import ExcerptsPanel from '../components/ExcerptsPanel.vue'
import CcEmpty from '../components/ui/CcEmpty.vue'
import CcButton from '../components/ui/CcButton.vue'
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { save } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../api'
import type { VideoCacheFull, VideoCacheInfo, VocabStats, VocabWord } from '../types'
import { videoKey } from '../utils/videokey'

/** 生词本：统计 + 筛选 + 表格（熟练度）+ 右侧今日复习卡 + 导出。设计图 ②。 */
// 生词 / 摘录（摘录以前只能在写作窗里看，浏览器扩展存的在“收到的文件”里）
const part = ref<'words' | 'excerpts'>('words')
const stats = ref<VocabStats>({ total: 0, due: 0, fresh: 0, learned: 0, today: 0 })
const all = ref<VocabWord[]>([])
const query = ref('')
const filter = ref<'all' | 'due' | 'new' | 'learned'>('all')
const source = ref('')
const loading = ref(false)
const sel = ref<number | null>(null)

// 复习
const queue = ref<VocabWord[]>([])
const total = ref(0)
const shown = ref(false)
const done = ref(0)
const current = computed(() => queue.value[0])

const NOW = () => Date.now() / 1000
const isDue = (w: VocabWord) => w.due <= NOW()
const isNew = (w: VocabWord) => !w.reps
const isLearned = (w: VocabWord) => w.reps >= 3 && w.intervalDays >= 21
/** 熟练度 0~4 格：没学过 0，间隔越长越满 */
function mastery(w: VocabWord): number {
  if (!w.reps) return 0
  if (w.intervalDays < 1) return 1
  if (w.intervalDays < 7) return 2
  if (w.intervalDays < 21) return 3
  return 4
}
const weekNew = computed(() => all.value.filter((w) => w.created >= NOW() - 7 * 86400).length)
const sources = computed(() => [...new Set(all.value.map((w) => w.sourceTitle).filter(Boolean))])
const dueCount = computed(() => all.value.filter(isDue).length)
const rows = computed(() => {
  const q = query.value.trim().toLowerCase()
  return all.value.filter((w) => {
    if (filter.value === 'due' && !isDue(w)) return false
    if (filter.value === 'new' && !isNew(w)) return false
    if (filter.value === 'learned' && !isLearned(w)) return false
    if (source.value && w.sourceTitle !== source.value) return false
    return !q || `${w.word} ${w.meaning} ${w.sentence}`.toLowerCase().includes(q)
  })
})
const picked = computed(() => all.value.find((w) => w.id === sel.value) ?? null)

// 视频缓存：扩展同步来的摘要和双语字幕
const caches = ref<Record<string, VideoCacheInfo>>({})
const cacheOf = (w: VocabWord) => (w.sourceUrl ? caches.value[videoKey(w.sourceUrl)] : undefined)
const cacheStats = ref({ videos: 0, bytes: 0 })
const autoSync = ref(true)
const limitMb = ref(500)
const viewing = ref<VideoCacheFull | null>(null)
const fmtMb = (n: number) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`)
const clock = (sec: number) => `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`
async function loadCaches() {
  const urls = [...new Set(all.value.map((w) => w.sourceUrl).filter(Boolean))]
  const list = urls.length ? await api.videocacheInfos(urls).catch(() => []) : [] // 忽略：可选数据，取不到就按空处理
  caches.value = Object.fromEntries(list.map((i) => [i.videoKey, i]))
  cacheStats.value = await api.videocacheStats().catch(() => ({ videos: 0, bytes: 0 }))
  const s = await api.getSettings().catch(() => null) // 忽略：可选数据，取不到就按空处理
  if (s) {
    autoSync.value = s.videoCacheAuto
    limitMb.value = s.videoCacheMb
  }
}
async function saveCacheSettings() {
  const s = await api.getSettings()
  await api.saveSettings({ ...s, videoCacheAuto: autoSync.value, videoCacheMb: Math.max(10, Number(limitMb.value) || 500) }, s)
}
/** 用默认浏览器打开出处：跳到那一秒；装了扩展会定位到那一句并高亮这个词。 */
async function openSource(w: VocabWord, restore = false) {
  if (!w.sourceUrl) return ElMessage.info('这个词没有记录出处网址')
  try {
    await api.videocacheOpen(w.sourceUrl, w.timeSec, w.word, restore)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}
async function viewCache(w: VocabWord) {
  viewing.value = await api.videocacheGet(w.sourceUrl).catch(() => null) // 忽略：可选数据，取不到就按空处理
  if (!viewing.value) ElMessage.info('清影里没有这个视频的缓存')
}
async function dropCache(w: VocabWord) {
  await api.videocacheRemove(w.sourceUrl)
  await loadCaches()
  ElMessage.success('已清除这个视频的缓存')
}
async function clearAllCache() {
  const r = await ElMessageBox.confirm('清除全部视频缓存？浏览器里的摘要和字幕缓存不受影响。', '清除缓存', { type: 'warning' }).catch(() => 'no')
  if (r === 'no') return
  await api.videocacheClear()
  await loadCaches()
}
const summaryText = computed(() => {
  const r = viewing.value?.summary?.data?.res as Record<string, unknown> | undefined
  const s = r?.summary
  return typeof s === 'string' ? s : s ? JSON.stringify(s, null, 1) : ''
})

async function refresh() {
  loading.value = true
  try {
    ;[all.value, stats.value] = await Promise.all([api.vocabList('', 'all'), api.vocabStats()])
    if (!reviewActive()) await loadQueue()
    void loadCaches()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}
const reviewActive = () => queue.value.length > 0 && done.value > 0
async function loadQueue() {
  const due = await api.vocabList('', 'due')
  queue.value = due
  total.value = due.length
  done.value = 0
  shown.value = false
}

/** 把例句里的这个词（含常见词形变化）挖空。返回 [前, 词, 后]。 */
function split(w: VocabWord): [string, string, string] | null {
  const s = w.sentence || ''
  if (!s) return null
  const esc = w.word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const m = new RegExp(`\\b${esc}\\w*`, 'i').exec(s)
  if (!m) return null
  return [s.slice(0, m.index), m[0], s.slice(m.index + m[0].length)]
}

async function grade(g: number) {
  const w = current.value
  if (!w) return
  try {
    const nw = await api.vocabReview(w.id, g)
    queue.value.shift()
    done.value++
    // “不认识”：10 分钟后再来，本轮末尾再问一次
    if (g === 0) queue.value.push(nw)
  } catch (e) {
    return ElMessage.error(errorText(e))
  }
  shown.value = false
  if (!queue.value.length) {
    ElMessage.success(`复习完成，共 ${done.value} 次`)
    done.value = 0
    void refresh()
  }
}

function onKey(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null
  if (!current.value || t?.tagName === 'INPUT') return
  if (e.key === ' ' || e.key === 'Enter') {
    e.preventDefault()
    shown.value = true
  } else if (shown.value && ['1', '2', '3'].includes(e.key)) void grade(Number(e.key) - 1)
}

function speak(text: string) {
  try {
    const u = new SpeechSynthesisUtterance(text)
    u.lang = 'en-US'
    speechSynthesis.cancel()
    speechSynthesis.speak(u)
  } catch {
    // 系统没有朗读
  }
}

async function remove(w: VocabWord) {
  const r = await ElMessageBox.confirm(`从生词本删除“${w.word}”？`, '删除', { type: 'warning' }).catch(() => 'no')
  if (r === 'no') return
  await api.vocabRemove(w.id)
  if (sel.value === w.id) sel.value = null
  void refresh()
}

const FORMATS = [
  ['anki', 'Anki', 'txt'],
  ['csv', 'CSV', 'csv'],
  ['md', 'Markdown', 'md'],
  ['json', 'JSON', 'json'],
] as const

const pushing = ref(false)
/** 直接发到本机正在运行的 Anki（AnkiConnect 插件）。 */
async function pushToAnki() {
  pushing.value = true
  try {
    const [added, skipped] = await api.vocabAnkiPush('清影生词')
    ElMessage.success(`已发送到 Anki：新增 ${added} 张${skipped ? `，${skipped} 张已存在或没加成` : ''}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    pushing.value = false
  }
}

async function exportAs(f: (typeof FORMATS)[number]) {
  const path = await save({ defaultPath: `生词本.${f[2]}`, filters: [{ name: f[1], extensions: [f[2]] }] })
  if (!path) return
  try {
    // Anki 而且录过原声时，后端存成 .zip（卡片 + 原声），返回实际写入的路径
    const out = await api.vocabExport(f[0], path)
    ElMessage.success(`已导出到 ${out}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

onMounted(() => {
  void refresh()
  window.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <div class="vocab dpage">
    <header class="h1 dtop">
      <h1>{{ part === 'words' ? '生词本' : '摘录' }}</h1>
      <p>{{ part === 'words' ? '来自你看过的视频字幕，每个词都带着原句和出处' : '浏览器里摘录、存为笔记的文字，写作窗、截图翻译里存的，都在这里' }}</p>
      <el-radio-group v-model="part" class="part dtabs">
        <el-radio-button value="words">生词</el-radio-button>
        <el-radio-button value="excerpts">摘录</el-radio-button>
      </el-radio-group>
    </header>
    <ExcerptsPanel v-if="part === 'excerpts'" class="expanel" />
    <template v-else>

    <div v-if="stats.total" class="stat-row">
      <div class="stat"><b>{{ stats.total }}</b><span>全部生词</span></div>
      <div class="stat hot"><b>{{ stats.due }}</b><span>今天该复习</span></div>
      <div class="stat"><b>{{ stats.learned }}</b><span>已掌握</span></div>
      <div class="stat"><b>{{ weekNew }}</b><span>本周新增</span></div>
    </div>

    <div class="body">
      <section class="listc">
        <div class="filters">
          <span class="chip" :class="{ on: filter === 'all' }" @click="filter = 'all'">全部</span>
          <span class="chip" :class="{ on: filter === 'due' }" @click="filter = 'due'">待复习 {{ dueCount }}</span>
          <span class="chip" :class="{ on: filter === 'new' }" @click="filter = 'new'">新词</span>
          <span class="chip" :class="{ on: filter === 'learned' }" @click="filter = 'learned'">已掌握</span>
          <el-select v-model="source" size="small" clearable placeholder="来源：全部视频" class="src-sel">
            <el-option v-for="s in sources" :key="s" :value="s" :label="s.slice(0, 40)" />
          </el-select>
          <el-input v-model="query" size="small" clearable placeholder="搜索单词或释义" class="search" />
        </div>
        <div v-if="rows.length" class="th"><span>单词</span><span>释义与原句</span><span>来源</span><span>熟练度</span></div>
        <CcEmpty v-if="!loading && !rows.length && stats.total" title="没有符合条件的词" desc="换个关键词，或放宽筛选条件试试。"><CcButton kind="secondary" size="s" @click="filter = 'all'; source = ''; query = ''">清除筛选</CcButton></CcEmpty>
        <CcEmpty v-else-if="!loading && !rows.length" title="还没有生词" desc="在浏览器里看视频，悬停字幕里的单词点“收藏”，就会出现在这里，可以导出。" />
        <div class="rowsc">
          <div v-for="w in rows" :key="w.id" class="tr" :class="{ sel: w.id === sel }" @click="sel = w.id" @dblclick="openSource(w)">
            <div>
              <b>{{ w.word }}</b><span class="ph">{{ w.phonetic }}</span>
              <span v-if="w.level" class="lv">{{ w.level }}</span>
            </div>
            <div>
              <div>{{ w.pos ? w.pos + ' ' : '' }}{{ w.meaning || '（没有释义）' }}<span v-if="w.ctxMeaning" class="mute">　本句：{{ w.ctxMeaning }}</span></div>
              <div v-if="w.sentence" class="src">{{ w.sentence }}</div>
            </div>
            <div class="src">
              <a v-if="w.sourceUrl" class="lnk" :title="`${w.sourceUrl}（双击整行也能打开）`" @click.stop="openSource(w)">{{ (w.sourceTitle || w.sourceUrl).slice(0, 28) }}<span class="tm">{{ clock(w.timeSec) }} ↗</span></a>
              <template v-else>{{ (w.sourceTitle || '').slice(0, 28) }}</template>
              <div v-if="cacheOf(w)" class="cbadge"><span v-if="cacheOf(w)!.hasSummary">摘要</span><span v-if="cacheOf(w)!.hasSubs">字幕</span></div>
            </div>
            <div class="dots"><i v-for="n in 4" :key="n" :class="{ f: n <= mastery(w) }" /></div>
          </div>
        </div>
        <div v-if="picked" class="pickbar">
          <b>{{ picked.word }}</b>
          <a class="act" @click="speak(picked.word)">朗读</a>
          <a v-if="picked.sourceUrl" class="act" @click="openSource(picked)">在浏览器打开出处</a>
          <template v-if="cacheOf(picked)">
            <a class="act" @click="viewCache(picked)">查看视频缓存</a>
            <a class="act" @click="openSource(picked, true)">打开视频并恢复缓存</a>
            <a class="act danger" @click="dropCache(picked)">清除缓存</a>
          </template>
          <a class="act danger" @click="remove(picked)">删除</a>
          <span class="mute">{{ picked.reps ? `已复习 ${picked.reps} 次` : '还没复习过' }}</span>
        </div>
      </section>

      <aside class="rev">
        <div class="fc">
          <template v-if="current">
            <div class="cap"><span>今日复习 {{ done + 1 }} / {{ total }}</span><span>{{ current.sourceTitle ? `来自 ${current.sourceTitle.slice(0, 26)}` : '' }}</span></div>
            <div v-if="split(current)" class="sentc">
              {{ split(current)![0] }}<span class="blank" :class="{ open: shown }">{{ shown ? split(current)![1] : '＿＿＿＿' }}</span>{{ split(current)![2] }}
            </div>
            <div v-else class="sentc">{{ current.meaning }}</div>
            <div v-if="shown" class="zh">
              <b>{{ current.word }}</b> <span class="ph">{{ current.phonetic }}</span>　{{ current.pos }} {{ current.meaning }}
              <div v-if="current.ctxMeaning" class="mute">本句：{{ current.ctxMeaning }}</div>
            </div>
            <div class="play" @click="speak(current.sentence || current.word)"><svg class="ic" viewBox="0 0 24 24" aria-hidden="true"><path d="M7 5l12 7-12 7z" /></svg>朗读这句</div>
            <el-button v-if="!shown" type="primary" @click="shown = true">显示答案（空格）</el-button>
            <div v-else class="grade">
              <div class="g" @click="grade(0)">不认识<span>{{ current.next[0] }}</span></div>
              <div class="g" @click="grade(1)">模糊<span>{{ current.next[1] }}</span></div>
              <div class="g p" @click="grade(2)">认识<span>{{ current.next[2] }}</span></div>
            </div>
          </template>
          <div v-else class="cap emp">{{ stats.total ? '今天没有要复习的词，明天再来。' : '收藏几个词后，这里每天出复习卡。' }}</div>
        </div>

        <div class="exp">
          <b>导出</b>
          <div class="btns">
            <el-button type="primary" size="small" :disabled="!stats.total" @click="exportAs(FORMATS[0])">导出到 Anki</el-button>
            <el-button size="small" :loading="pushing" :disabled="!stats.total" @click="pushToAnki">直接发送到 Anki</el-button>
            <el-button v-for="f in FORMATS.slice(1)" :key="f[0]" size="small" :disabled="!stats.total" @click="exportAs(f)">{{ f[1] }}</el-button>
          </div>
          <div class="hint">Anki 卡片正面是挖空原句，背面是释义和出处；在浏览器里的“生词”页也能一键导出。“直接发送”需要先打开 Anki 并装好 AnkiConnect 插件。</div>
        </div>

        <div class="exp">
          <b>视频缓存</b>
          <div class="hint">浏览器扩展里生成的摘要和双语字幕，收藏生词时同步一份到这里。现有 {{ cacheStats.videos }} 个视频，占用 {{ fmtMb(cacheStats.bytes) }}。</div>
          <el-switch v-model="autoSync" size="small" active-text="收藏生词的视频自动同步" @change="saveCacheSettings" />
          <div class="btns">上限 <el-input-number v-model="limitMb" size="small" :min="10" :max="5000" :step="100" controls-position="right" @change="saveCacheSettings" /> MB
            <el-button size="small" :disabled="!cacheStats.videos" @click="clearAllCache">清除全部</el-button></div>
        </div>
      </aside>
    </div>

    <el-dialog append-to-body :model-value="!!viewing" :title="viewing ? (viewing.info.title || '视频缓存') : ''" width="760px" @close="viewing = null">
      <template v-if="viewing">
        <div class="mute">{{ viewing.info.url }}</div>
        <h4 v-if="summaryText">摘要</h4>
        <pre v-if="summaryText" class="cachetxt">{{ summaryText }}</pre>
        <h4 v-if="viewing.subs?.lines?.length">双语字幕（共 {{ viewing.subs.lines.length }} 句，显示前 120 句）</h4>
        <div v-if="viewing.subs?.lines?.length" class="cachesubs">
          <div v-for="(l, i) in viewing.subs.lines.slice(0, 120)" :key="i"><span class="tm">{{ clock(l.t / 1000) }}</span>{{ l.text }}<div v-if="viewing.subs.trans?.[i]" class="mute">{{ viewing.subs.trans[i] }}</div></div>
        </div>
        <div v-if="!summaryText && !viewing.subs?.lines?.length" class="mute">缓存里没有可显示的内容。</div>
      </template>
    </el-dialog>
    </template>
  </div>
</template>

<style scoped>
.vocab { height: 100%; box-sizing: border-box; min-height: 0 }
.h1 { display: flex; align-items: flex-end; gap: 14px; flex-wrap: wrap }
.h1 h1 { margin: 0; font-size: 22px }
.h1 p { margin: 0; color: var(--cc-mute) }
.h1 .part { margin-left: auto }
.expanel { flex: 1; min-height: 0 }
.stat-row { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px }
.stat { border: 1px solid var(--cc-line); background: var(--cc-panel2); border-radius: 12px; padding: 14px 16px; display: flex; flex-direction: column; gap: 2px }
.stat b { font-size: 28px; font-family: var(--cc-disp) }
.stat span { color: var(--cc-mute); font-size: 12.5px }
.stat.hot { border-color: var(--cc-amber); background: var(--cc-acc-soft) }
.stat.hot b { color: var(--cc-amber) }
.body { flex: 1; display: flex; gap: 18px; min-height: 0 }
.listc { flex: 1; min-width: 0; border: 1px solid var(--cc-line); background: var(--cc-card); border-radius: 12px; display: flex; flex-direction: column; overflow: hidden }
.filters { display: flex; gap: 8px; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--cc-line); flex-wrap: wrap }
.chip { height: 26px; padding: 0 11px; border-radius: 13px; border: 1px solid var(--cc-line); display: flex; align-items: center; font-size: 12.5px; color: var(--cc-mute); cursor: pointer }
.chip.on { background: var(--cc-acc-soft); border-color: var(--cc-amber); color: var(--cc-amber); font-weight: 700 }
.src-sel { width: 170px }
.search { width: 170px; margin-left: auto }
.th, .tr { display: grid; grid-template-columns: 1.1fr 2.2fr 1.3fr 84px; gap: 12px; padding: 8px 14px; align-items: center }
.th { font-size: 12px; color: var(--cc-mute); border-bottom: 1px solid var(--cc-line); background: var(--cc-fill) }
.rowsc { flex: 1; overflow-y: auto }
.tr { border-bottom: 1px solid var(--cc-line-soft); cursor: pointer }
.tr:hover { background: var(--cc-hover) }
.tr.sel { background: var(--cc-acc-soft) }
.ph { color: var(--cc-mute); font-size: 12px; margin-left: 6px }
.lv { font-size: 11px; padding: 0 7px; border-radius: 9px; border: 1px solid var(--cc-amber); color: var(--cc-amber); margin-left: 6px }
.src { color: var(--cc-mute); font-size: 12.5px; line-height: 1.5 }
.tm { margin-left: 8px; color: var(--cc-amber); font-variant-numeric: tabular-nums }
.mute { color: var(--cc-mute); font-size: 12.5px }
.dots { display: flex; gap: 4px }
.dots i { width: 10px; height: 10px; border-radius: 50%; background: var(--cc-line) }
.dots i.f { background: var(--cc-amber) }
.pickbar { display: flex; gap: 14px; align-items: center; padding: 8px 14px; border-top: 1px solid var(--cc-line) }
.act { cursor: pointer; color: var(--cc-amber); font-size: 12.5px }
.act.danger { color: var(--cc-err) }
.rev { width: 340px; flex: none; display: flex; flex-direction: column; gap: 12px }
.fc { border: 1px solid var(--cc-line); background: var(--cc-card); border-radius: 12px; padding: 16px; display: flex; flex-direction: column; gap: 12px }
.cap { display: flex; justify-content: space-between; gap: 8px; font-size: 12px; color: var(--cc-mute) }
.cap.emp { justify-content: center; padding: 24px 0; font-size: 13px }
.sentc { font-size: 17px; line-height: 1.65 }
.blank { display: inline-block; min-width: 70px; text-align: center; border-bottom: 2px solid var(--cc-amber); color: var(--cc-amber); font-weight: 700 }
.blank.open { border-bottom-style: dotted }
.zh { color: var(--cc-fg2); font-size: 13.5px; line-height: 1.6 }
.play { display: flex; align-items: center; gap: 6px; color: var(--cc-amber); font-size: 13px; cursor: pointer }
.ic { width: 16px; height: 16px; fill: currentColor }
.grade { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px }
.g { border: 1px solid var(--cc-line); border-radius: 10px; padding: 8px 4px; text-align: center; cursor: pointer; display: flex; flex-direction: column; gap: 2px; font-weight: 600 }
.g span { font-weight: 400; font-size: 11px; color: var(--cc-mute) }
.g.p { background: var(--cc-amber); border-color: var(--cc-amber); color: var(--cc-on-acc) }
.g.p span { color: var(--cc-on-acc); opacity: .85 }
.exp { border: 1px solid var(--cc-line); background: var(--cc-card); border-radius: 12px; padding: 12px 14px; display: flex; flex-direction: column; gap: 8px }
.btns { display: flex; gap: 6px; flex-wrap: wrap }
.hint { color: var(--cc-mute); font-size: 12px; line-height: 1.5 }
@media (max-width: 1000px) { .body { flex-direction: column } .rev { width: auto } }
.lnk { color: var(--cc-amber); cursor: pointer; text-decoration: underline; text-underline-offset: 2px }
.cbadge { display: flex; gap: 4px; margin-top: 2px }
.cbadge span { font-size: 11px; padding: 0 7px; border-radius: 9px; background: var(--cc-cyan-soft); color: var(--cc-cyan) }
.cachetxt { white-space: pre-wrap; max-height: 220px; overflow: auto; background: var(--cc-hover); padding: 10px 12px; border-radius: 8px; font-family: inherit; font-size: 13px; line-height: 1.7 }
.cachesubs { max-height: 300px; overflow: auto; font-size: 13px; line-height: 1.7 }
</style>
