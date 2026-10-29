<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import type { CueHit } from '../../types'
import { formatClock } from '../../utils/time'
import { useAppStore } from '../../stores/app'

const app = useAppStore()

const query = ref('')
const hits = ref<CueHit[]>([])
const searched = ref(false)
const busy = ref(false)
const indexing = ref(false)
let timer: number | undefined

async function search() {
  const q = query.value.trim()
  if (!q) {
    hits.value = []
    searched.value = false
    return
  }
  busy.value = true
  try {
    hits.value = await api.cuesSearch(q)
    searched.value = true
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}

function onInput() {
  window.clearTimeout(timer)
  timer = window.setTimeout(search, 300)
}

async function reindex() {
  indexing.value = true
  try {
    const n = await api.cuesReindex()
    ElMessage.success(`已为 ${n} 条字幕建立索引`)
    await search()
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    indexing.value = false
  }
}

function parts(text: string) {
  const q = query.value.trim()
  if (!q) return [{ t: text, hit: false }]
  const out: { t: string; hit: boolean }[] = []
  const lower = text.toLowerCase()
  const ql = q.toLowerCase()
  let i = 0
  while (i < text.length) {
    const at = lower.indexOf(ql, i)
    if (at < 0) {
      out.push({ t: text.slice(i), hit: false })
      break
    }
    if (at > i) out.push({ t: text.slice(i, at), hit: false })
    out.push({ t: text.slice(at, at + q.length), hit: true })
    i = at + q.length
  }
  return out
}

/** 用内置播放器直接跳到这句话出现的位置。 */
function openAt(h: CueHit) {
  app.openPlayer(h.path, h.startMs)
}

async function openExternal(h: CueHit) {
  try {
    await api.copyText(formatClock(h.startMs))
    await api.openFile(h.path)
    ElMessage.info(`已用系统播放器打开，位置 ${formatClock(h.startMs)} 已复制到剪贴板`)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
}

</script>

<template>
  <div class="cues">
    <div class="bar">
      <el-input v-model="query" size="small" clearable placeholder="在所有已下载的字幕里搜索一句话…" @input="onInput" @keyup.enter="search" />
      <el-button size="small" :loading="indexing" @click="reindex">重建索引</el-button>
    </div>
    <p class="mute tip">可以搜索 SRT、VTT、ASS 字幕里的文字，找到视频里说过这句话的位置。新下载或导入的字幕会自动建立索引；弹幕不参与搜索。</p>
    <div v-loading="busy" class="res">
      <div v-if="searched && !hits.length" class="mute empty">没有找到包含“{{ query }}”的字幕。</div>
      <div v-for="(h, i) in hits" :key="i" class="hit">
        <div class="meta">
          <span class="ellipsis" :title="h.title || h.path">{{ h.title || '（视频已不在媒体库里）' }}</span>
          <span class="mono mute">{{ formatClock(h.startMs) }}</span>
          <span v-if="h.lang && h.lang !== 'und'" class="chip">{{ h.lang }}</span>
          <el-button link type="primary" size="small" :disabled="!h.path" @click="openAt(h)">播放</el-button>
          <el-button link size="small" :disabled="!h.path" @click="openExternal(h)">系统播放器</el-button>
        </div>
        <div class="text selectable"><template v-for="(p, j) in parts(h.text)" :key="j"><mark v-if="p.hit">{{ p.t }}</mark><template v-else>{{ p.t }}</template></template></div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.cues {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.bar {
  display: flex;
  gap: 8px;
}
.tip {
  margin: 0;
  font-size: 11.5px;
}
.res {
  min-height: 100px;
  max-height: 420px;
  overflow: auto;
}
.hit {
  padding: 6px 0;
  border-bottom: 1px solid var(--cc-line);
}
.meta {
  display: grid;
  grid-template-columns: 1fr auto auto auto;
  gap: 10px;
  align-items: center;
  font-size: 12px;
}
.text {
  font-size: 13px;
  margin-top: 2px;
}
mark {
  background: var(--cc-acc-soft);
  color: var(--cc-acc);
  border-radius: 2px;
  padding: 0 1px;
}
.empty {
  padding: 30px 0;
  text-align: center;
}
</style>
