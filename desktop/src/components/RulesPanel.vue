<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api } from '../api'
import { useAppStore } from '../stores/app'
import type { AutoRule, NormPreset } from '../types'

const rules = defineModel<AutoRule[]>({ required: true })
const app = useAppStore()
const platforms = computed(() => app.info?.providers ?? [])
const presets = ref<NormPreset[]>([])
onMounted(async () => (presets.value = await api.videoPresets().catch(() => [])))

function add() {
  rules.value.push({
    id: Math.random().toString(36).slice(2, 10),
    name: `规则 ${rules.value.length + 1}`,
    enabled: true,
    when: { platform: '', author: '', title: '', kind: 'video', source: '', minSizeMb: 0 },
    then: { addTags: [], favorite: false, moveTo: '', extractAudio: '', upload: false, notify: false, normalize: '' },
  })
}

function move(i: number, d: number) {
  const j = i + d
  if (j < 0 || j >= rules.value.length) return
  const next = [...rules.value]
  ;[next[i], next[j]] = [next[j], next[i]]
  rules.value = next
}
</script>

<template>
  <section class="group card">
    <h3>自动规则</h3>
    <small class="mute">每个文件下载完成后，按顺序检查下面的规则：条件都满足就执行动作（可以同时命中多条）。文字条件用 <span class="mono">|</span> 分隔多个关键词，满足任意一个即可；以 <span class="mono">re:</span> 开头表示正则。</small>
    <div v-for="(r, i) in rules" :key="r.id" class="rule">
      <div class="top">
        <el-switch v-model="r.enabled" size="small" />
        <el-input v-model="r.name" size="small" class="name" />
        <el-button link size="small" :disabled="i === 0" @click="move(i, -1)">上移</el-button>
        <el-button link size="small" :disabled="i === rules.length - 1" @click="move(i, 1)">下移</el-button>
        <el-button link size="small" @click="rules.splice(i, 1)">删除</el-button>
      </div>
      <div class="cols">
        <div class="col">
          <h4>当…</h4>
          <label>平台</label>
          <el-select v-model="r.when.platform" size="small" clearable filterable allow-create placeholder="不限">
            <el-option v-for="p in platforms" :key="p.id" :value="p.id" :label="p.name" />
          </el-select>
          <label>作者包含</label>
          <el-input v-model="r.when.author" size="small" placeholder="不限" />
          <label>标题包含</label>
          <el-input v-model="r.when.title" size="small" placeholder="不限，如 教程|入门" />
          <label>类型 / 来源</label>
          <div class="two">
            <el-select v-model="r.when.kind" size="small" clearable placeholder="类型不限">
              <el-option value="video" label="视频" /><el-option value="audio" label="音频" /><el-option value="image" label="图片" /><el-option value="subtitle" label="字幕" />
            </el-select>
            <el-select v-model="r.when.source" size="small" clearable placeholder="来源不限">
              <el-option value="manual" label="手动下载" /><el-option value="subscription" label="订阅" /><el-option value="live" label="直播录制" /><el-option value="phone" label="手机发送" />
            </el-select>
          </div>
          <label>文件大小不小于（MB）</label>
          <el-input-number v-model="r.when.minSizeMb" size="small" :min="0" controls-position="right" />
        </div>
        <div class="col">
          <h4>就…</h4>
          <label>打标签</label>
          <el-input-tag v-model="r.then.addTags" size="small" placeholder="输入后回车" />
          <el-checkbox v-model="r.then.favorite" size="small">加入收藏</el-checkbox>
          <label>移动到子目录</label>
          <el-input v-model="r.then.moveTo" size="small" class="mono" placeholder="如 学习/{author}（留空不移动）" />
          <label>提取音频</label>
          <el-select v-model="r.then.extractAudio" size="small" clearable placeholder="不提取">
            <el-option value="mp3" label="MP3" /><el-option value="m4a" label="M4A" /><el-option value="flac" label="FLAC" /><el-option value="opus" label="Opus" />
          </el-select>
          <label>视频规整</label>
          <el-select v-model="r.then.normalize" size="small" clearable placeholder="不规整">
            <el-option v-for="p in presets" :key="p.id" :value="p.id" :label="p.name" />
          </el-select>
          <el-checkbox v-model="r.then.upload" size="small">上传（需先在“自动上传”里填好目标）</el-checkbox>
          <el-checkbox v-model="r.then.notify" size="small">推送一条通知</el-checkbox>
        </div>
      </div>
    </div>
    <div><el-button size="small" @click="add">添加规则</el-button></div>
  </section>
</template>

<style scoped>
.group {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}
h3 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-mute);
}
h4 {
  margin: 0;
  font-size: 12px;
  color: var(--cc-acc);
}
.rule {
  border: 1px solid var(--cc-line);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.top {
  display: flex;
  gap: 8px;
  align-items: center;
}
.name {
  flex: 1;
}
.cols {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
}
.col {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}
.col label {
  font-size: 11.5px;
  color: var(--cc-mute);
}
.two {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
</style>
