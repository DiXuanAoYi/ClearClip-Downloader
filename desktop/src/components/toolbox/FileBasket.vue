<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api } from '../../api'
import { formatDuration } from '../../utils/format'
import { kindOf } from '../../utils/toolbox'
import FileInput from './FileInput.vue'

const files = defineModel<string[]>({ required: true })
const props = defineProps<{
  /** 当前工具能处理的文件，不在里面的文件在篮里淡显 */
  accepts: string[]
  /** 当前工具用到的文件个数提示，如“压缩 3 个文件” */
  note?: string
}>()

const LIMIT = 8
const expanded = ref(false)
const shown = computed(() => (expanded.value ? files.value : files.value.slice(0, LIMIT)))
const name = (p: string) => p.split(/[\\/]/).pop() ?? p

interface Meta {
  line: string
  thumb?: string
}
const metas = reactive<Record<string, Meta>>({})

async function load(p: string) {
  if (metas[p]) return
  metas[p] = { line: '' }
  const kind = kindOf(p)
  try {
    const i = await api.mediaInfo(p)
    const parts: string[] = []
    if (i.width) parts.push(`${i.width}×${i.height}`)
    if (i.durationMs) parts.push(formatDuration(i.durationMs))
    metas[p] = { line: parts.join(' · '), thumb: kind === 'image' ? convertFileSrc(p) : undefined }
  } catch {
    metas[p] = { line: kind === 'subtitle' ? '字幕' : '' , thumb: kind === 'image' ? convertFileSrc(p) : undefined }
  }
}
watch(
  shown,
  (list) => {
    for (const p of list) void load(p)
  },
  { immediate: true },
)

const usable = (p: string) => props.accepts.includes(kindOf(p))
const skipped = computed(() => files.value.filter((p) => !usable(p)).length)

function remove(p: string) {
  files.value = files.value.filter((x) => x !== p)
}
function move(i: number, d: number) {
  const next = [...files.value]
  const j = i + d
  if (j < 0 || j >= next.length) return
  ;[next[i], next[j]] = [next[j], next[i]]
  files.value = next
}
</script>

<template>
  <section class="basket" aria-label="文件篮">
    <div class="head">
      <strong>文件篮</strong>
      <span class="count" :class="{ zero: !files.length }">{{ files.length }}</span>
      <span class="hint mute">选一次，换到任何工具都还在；处理时每个文件各生成一个新文件。</span>
      <FileInput v-model="files" multiple compact plain browse-text="添加文件…" library-text="从媒体库选" :kinds="['video', 'audio', 'image']" label="媒体文件" />
      <el-button v-if="files.length" link size="small" @click="files = []">清空</el-button>
    </div>
    <div v-if="files.length" class="chips">
      <div v-for="(p, i) in shown" :key="p" class="chip" :class="{ off: !usable(p), first: i === 0 }">
        <div class="thumb"><img v-if="metas[p]?.thumb" :src="metas[p].thumb" alt="" /></div>
        <div class="txt">
          <div class="nm ellipsis" :title="p">{{ name(p) }}</div>
          <div class="meta mute">{{ usable(p) ? metas[p]?.line || '读取中…' : '这个工具用不到' }}</div>
        </div>
        <div v-if="files.length > 1" class="mv">
          <button type="button" aria-label="前移" :disabled="i === 0" @click="move(i, -1)">‹</button>
          <button type="button" aria-label="后移" :disabled="i === files.length - 1" @click="move(i, 1)">›</button>
        </div>
        <button type="button" class="x" aria-label="移出文件篮" @click="remove(p)">×</button>
      </div>
      <button v-if="files.length > LIMIT" type="button" class="more" @click="expanded = !expanded">{{ expanded ? '收起' : `还有 ${files.length - LIMIT} 个` }}</button>
    </div>
    <div v-if="files.length && (note || skipped)" class="foot mute">
      <span v-if="note">{{ note }}</span>
      <span v-if="skipped">有 {{ skipped }} 个文件这个工具用不到，会被跳过。</span>
    </div>
  </section>
</template>

<style scoped>
.basket {
  background: var(--cc-card, var(--cc-bg));
  border: 1px solid var(--cc-line);
  border-radius: 10px;
  padding: 10px 14px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.hint {
  flex: 1;
  min-width: 200px;
  font-size: 12px;
}
.count {
  background: var(--cc-acc);
  color: #fff;
  font-size: 12px;
  font-weight: 600;
  padding: 0 8px;
  border-radius: 10px;
  line-height: 18px;
}
.count.zero {
  background: var(--cc-line);
  color: var(--cc-mute);
}
.chips {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 8px;
}
.chip {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  min-width: 0;
}
.chip.first {
  border-color: var(--cc-acc);
}
.chip.off {
  opacity: 0.5;
}
.thumb {
  width: 34px;
  height: 34px;
  flex: none;
  border-radius: 5px;
  background: var(--cc-line);
  overflow: hidden;
}
.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.txt {
  flex: 1;
  min-width: 0;
}
.nm {
  font-weight: 600;
  font-size: 12.5px;
}
.meta {
  font-size: 11.5px;
}
.mv {
  display: flex;
}
.mv button,
.x {
  all: unset;
  cursor: pointer;
  width: 20px;
  height: 20px;
  text-align: center;
  line-height: 20px;
  border-radius: 10px;
  color: var(--cc-mute);
}
.mv button:disabled {
  opacity: 0.3;
  cursor: default;
}
.mv button:hover:not(:disabled),
.x:hover {
  background: var(--cc-acc-soft);
  color: var(--cc-fg);
}
.mv button:focus-visible,
.x:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.more {
  all: unset;
  cursor: pointer;
  align-self: center;
  color: var(--cc-acc);
  font-size: 12.5px;
  padding: 0 8px;
}
.foot {
  display: flex;
  gap: 14px;
  font-size: 12px;
  flex-wrap: wrap;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
