<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import type { MediaInfoLite } from '../../types'
import FileInput from './FileInput.vue'

const files = ref<string[]>([])
const cover = ref<string[]>([])
const info = ref<MediaInfoLite | null>(null)
const busy = ref(false)
const f = reactive({ title: '', artist: '', album: '', year: '', genre: '', comment: '' })
const orig = reactive({ title: '', artist: '', album: '', year: '', genre: '', comment: '' })

watch(files, async (list) => {
  info.value = null
  cover.value = []
  for (const k of Object.keys(f) as (keyof typeof f)[]) f[k] = orig[k] = ''
  if (!list.length) return
  try {
    const i = await api.mediaInfo(list[0])
    info.value = i
    const t = i.tags
    const get = (k: string) => t[k] ?? ''
    Object.assign(orig, { title: get('title'), artist: get('artist'), album: get('album'), year: get('date') || get('year'), genre: get('genre'), comment: get('comment') })
    Object.assign(f, orig)
  } catch (e) {
    ElMessage.error(errorText(e))
  }
})

async function save() {
  if (!files.value.length) return
  busy.value = true
  try {
    // 没改的不传（保持原样），改成空的传空字符串（清除）
    const pick = (k: keyof typeof f) => (f[k] !== orig[k] ? f[k] : null)
    await api.mediaJobStart({
      op: 'tags',
      inputs: files.value,
      title: pick('title'),
      artist: pick('artist'),
      album: pick('album'),
      year: pick('year'),
      genre: pick('genre'),
      comment: pick('comment'),
      cover: cover.value[0] ?? null,
    })
    ElMessage.success('已开始处理，会保存为新文件，原文件不会被修改。')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="tt card">
    <h3>音频标签</h3>
    <p class="mute desc">给音乐文件（MP3、M4A、FLAC 等）写入标题、艺术家、专辑和封面，保存为新文件，不重新编码。</p>
    <FileInput v-model="files" :extensions="['mp3', 'm4a', 'flac', 'opus', 'ogg', 'wav', 'mp4']" :kinds="['audio']" label="音频文件" />
    <div v-if="info" class="form">
      <label><span>标题</span><el-input v-model="f.title" size="small" /></label>
      <label><span>艺术家</span><el-input v-model="f.artist" size="small" /></label>
      <label><span>专辑</span><el-input v-model="f.album" size="small" /></label>
      <label><span>年份</span><el-input v-model="f.year" size="small" class="short" /></label>
      <label><span>风格</span><el-input v-model="f.genre" size="small" class="short" /></label>
      <label><span>备注</span><el-input v-model="f.comment" size="small" /></label>
      <label>
        <span>封面图片</span>
        <FileInput v-model="cover" :extensions="['jpg', 'jpeg', 'png', 'webp']" label="封面图片" />
      </label>
      <small class="mute">只支持 MP3、M4A、FLAC、MP4 写入封面。把某一项清空后保存，会清除这一项。</small>
    </div>
    <div class="go"><el-button type="primary" :loading="busy" :disabled="!info" @click="save">保存为新文件</el-button></div>
  </div>
</template>

<style scoped>
.tt {
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: 640px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
.desc {
  margin: 0;
  font-size: 12.5px;
}
.form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.form label {
  display: grid;
  grid-template-columns: 70px 1fr;
  gap: 10px;
  align-items: center;
}
.form label > span:first-child {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.short {
  width: 160px;
}
</style>
