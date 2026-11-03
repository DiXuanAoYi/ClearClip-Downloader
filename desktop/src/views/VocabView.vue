<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { save } from '@tauri-apps/plugin-dialog'
import { api, errorText } from '../api'
import type { VocabStats, VocabWord } from '../types'
import { formatDateTime } from '../utils/format'

const stats = ref<VocabStats>({ total: 0, due: 0, fresh: 0, learned: 0, today: 0 })
const words = ref<VocabWord[]>([])
const query = ref('')
const filter = ref<'all' | 'due' | 'new' | 'learned'>('all')
const loading = ref(false)

// 复习
const queue = ref<VocabWord[]>([])
const reviewing = ref(false)
const shown = ref(false)
const done = ref(0)
const current = computed(() => queue.value[0])

async function refresh() {
  loading.value = true
  try {
    ;[words.value, stats.value] = await Promise.all([api.vocabList(query.value.trim(), filter.value), api.vocabStats()])
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    loading.value = false
  }
}

let t = 0
function onSearch() {
  clearTimeout(t)
  t = window.setTimeout(refresh, 250)
}

async function startReview() {
  const due = await api.vocabList('', 'due')
  if (!due.length) return ElMessage.info('现在没有要复习的词')
  queue.value = due
  done.value = 0
  shown.value = false
  reviewing.value = true
}

/** 把例句里的这个词（含常见词形变化）挖空。 */
function cloze(w: VocabWord) {
  const s = w.sentence || ''
  if (!s) return ''
  const esc = w.word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return s.replace(new RegExp(`\\b${esc}\\w*`, 'gi'), '＿＿＿＿')
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
    reviewing.value = false
    ElMessage.success(`复习完成，共 ${done.value} 次`)
    refresh()
  }
}

function onKey(e: KeyboardEvent) {
  if (!reviewing.value) return
  if (e.key === ' ' || e.key === 'Enter') {
    e.preventDefault()
    shown.value = true
  } else if (shown.value && ['1', '2', '3'].includes(e.key)) grade(Number(e.key) - 1)
  else if (e.key === 'Escape') {
    reviewing.value = false
    refresh()
  }
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
  await ElMessageBox.confirm(`从生词本删除“${w.word}”？`, '删除', { type: 'warning' }).catch(() => 'no')
    .then(async (r) => {
      if (r === 'no') return
      await api.vocabRemove(w.id)
      refresh()
    })
}

const FORMATS = [
  ['anki', 'Anki（TSV）', 'txt'],
  ['csv', 'CSV', 'csv'],
  ['md', 'Markdown', 'md'],
  ['json', 'JSON', 'json'],
] as const

async function exportAs(f: (typeof FORMATS)[number]) {
  const path = await save({ defaultPath: `生词本.${f[2]}`, filters: [{ name: f[1], extensions: [f[2]] }] })
  if (!path) return
  try {
    await api.vocabExport(f[0], path)
    ElMessage.success(`已导出到 ${path}`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

const FILTERS = [
  ['all', '全部'],
  ['due', '该复习'],
  ['new', '没学过'],
  ['learned', '已掌握'],
] as const

onMounted(() => {
  refresh()
  window.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <div class="vocab">
    <header class="head">
      <div>
        <h2>生词本</h2>
        <p class="sub">在浏览器看视频时悬停字幕里的单词就能收藏；这里复习、导出。</p>
      </div>
      <div class="stats">
        <span><b>{{ stats.total }}</b> 个词</span>
        <span class="due"><b>{{ stats.due }}</b> 该复习</span>
        <span><b>{{ stats.learned }}</b> 已掌握</span>
        <span>今天已复习 <b>{{ stats.today }}</b></span>
      </div>
      <el-button type="primary" :disabled="!stats.due" @click="startReview">开始复习（{{ stats.due }}）</el-button>
    </header>

    <div class="bar">
      <el-input v-model="query" placeholder="搜索单词 / 释义 / 例句" clearable style="max-width: 280px" @input="onSearch" />
      <el-radio-group v-model="filter" size="small" @change="refresh">
        <el-radio-button v-for="f in FILTERS" :key="f[0]" :value="f[0]">{{ f[1] }}</el-radio-button>
      </el-radio-group>
      <span class="grow" />
      <el-dropdown trigger="click">
        <el-button>导出<span style="margin-left: 4px">▾</span></el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item v-for="f in FORMATS" :key="f[0]" @click="exportAs(f)">{{ f[1] }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>

    <el-empty v-if="!loading && !words.length" :description="stats.total ? '没有符合条件的词' : '还没有收藏的词。在浏览器里看视频，悬停字幕里的单词点“收藏”就会出现在这里。'" />
    <div v-else class="list">
      <div v-for="w in words" :key="w.id" class="card">
        <div class="top">
          <b class="w">{{ w.word }}</b>
          <span class="ph">{{ w.phonetic }}</span>
          <span v-if="w.level" class="lv">{{ w.level }}</span>
          <span class="grow" />
          <a class="act" @click="speak(w.word)">朗读</a>
          <a class="act danger" @click="remove(w)">删除</a>
        </div>
        <div class="mean"><i v-if="w.pos">{{ w.pos }}</i>{{ w.meaning || '（没有释义，可在查词气泡里重新收藏）' }}</div>
        <div v-if="w.ctxMeaning" class="ctx">本句：{{ w.ctxMeaning }}</div>
        <div v-if="w.sentence" class="sent">{{ w.sentence }}</div>
        <div class="meta">
          <span v-if="w.sourceTitle">{{ w.sourceTitle.slice(0, 40) }}</span>
          <span>{{ w.reps ? `下次复习 ${formatDateTime(w.due * 1000)}` : '还没复习过' }}</span>
        </div>
      </div>
    </div>

    <el-dialog v-model="reviewing" width="520px" :show-close="true" title="复习" align-center destroy-on-close>
      <div v-if="current" class="rv">
        <div class="prog">还剩 {{ queue.length }} 个 · 空格翻面，1 / 2 / 3 评分，Esc 退出</div>
        <div class="front">
          <template v-if="current.sentence">{{ cloze(current) }}</template>
          <template v-else>{{ current.meaning }}</template>
        </div>
        <template v-if="shown">
          <div class="back">
            <b>{{ current.word }}</b> <span class="ph">{{ current.phonetic }}</span>
            <a class="act" @click="speak(current.word)">朗读</a>
            <div class="mean"><i v-if="current.pos">{{ current.pos }}</i>{{ current.meaning }}</div>
            <div v-if="current.ctxMeaning" class="ctx">本句：{{ current.ctxMeaning }}</div>
            <div v-if="current.sentence" class="sent">{{ current.sentence }}</div>
          </div>
          <div class="grades">
            <el-button @click="grade(0)">不认识<small>{{ current.next[0] }}</small></el-button>
            <el-button @click="grade(1)">模糊<small>{{ current.next[1] }}</small></el-button>
            <el-button type="primary" @click="grade(2)">认识<small>{{ current.next[2] }}</small></el-button>
          </div>
        </template>
        <el-button v-else type="primary" class="flip" @click="shown = true">显示答案</el-button>
      </div>
    </el-dialog>
  </div>
</template>

<style scoped>
.vocab { display: flex; flex-direction: column; gap: 14px; padding: 20px 24px; height: 100%; overflow-y: auto; box-sizing: border-box }
.head { display: flex; align-items: center; gap: 20px; flex-wrap: wrap }
.head h2 { margin: 0 }
.sub { margin: 4px 0 0; color: var(--el-text-color-secondary); font-size: 13px }
.stats { display: flex; gap: 16px; margin-left: auto; color: var(--el-text-color-secondary); font-size: 13px }
.stats b { color: var(--el-text-color-primary); font-size: 18px }
.stats .due b { color: var(--el-color-warning) }
.bar { display: flex; align-items: center; gap: 12px; flex-wrap: wrap }
.grow { flex: 1 }
.list { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 12px }
.card { border: 1px solid var(--el-border-color); border-radius: 10px; padding: 12px 14px; display: flex; flex-direction: column; gap: 5px; background: var(--el-bg-color) }
.top { display: flex; align-items: baseline; gap: 8px }
.w { font-size: 18px }
.ph { color: var(--el-text-color-secondary); font-size: 12px }
.lv { font-size: 11px; padding: 0 7px; border-radius: 9px; border: 1px solid var(--el-color-warning); color: var(--el-color-warning) }
.act { cursor: pointer; color: var(--el-color-primary); font-size: 12px }
.act.danger { color: var(--el-color-danger) }
.mean i, .back i { font-style: normal; color: var(--el-text-color-secondary); margin-right: 6px }
.ctx { padding: 5px 8px; border-radius: 6px; background: var(--el-fill-color-light); font-size: 13px }
.sent { color: var(--el-text-color-secondary); font-size: 12.5px; line-height: 1.5 }
.meta { display: flex; justify-content: space-between; gap: 8px; color: var(--el-text-color-placeholder); font-size: 11.5px }
.rv { display: flex; flex-direction: column; gap: 14px }
.prog { color: var(--el-text-color-secondary); font-size: 12px }
.front { font-size: 18px; line-height: 1.6; min-height: 70px }
.back { display: flex; flex-direction: column; gap: 6px; border-top: 1px solid var(--el-border-color); padding-top: 12px }
.grades { display: flex; gap: 10px }
.grades .el-button { flex: 1; height: 52px; flex-direction: column; gap: 2px; margin: 0 }
.grades small { display: block; opacity: 0.75; font-size: 11px }
.flip { align-self: center }
</style>
