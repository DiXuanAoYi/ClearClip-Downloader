<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../../api'
import type { SubJob, SubOp } from '../../types'
import { parseClock } from '../../utils/time'
import FileInput from './FileInput.vue'

const OPS = [
  { id: 'shift', name: '平移时间轴', desc: '字幕整体提前或推后几秒。' },
  { id: 'align', name: '两点校准', desc: '前面对得上、越到后面越对不上时，给出开头和结尾各一个对应的时间点。' },
  { id: 'rescale', name: '按比例缩放', desc: '帧率不一致造成的逐渐偏移，例如 23.976 → 25 帧。' },
  { id: 'merge', name: '合并双语', desc: '把两个语言的字幕合成一个，第二种语言显示在下面。' },
  { id: 'clean', name: '清理标记', desc: '去掉 HTML / 样式标记，可选去掉 [音乐]（笑）这类说明。' },
  { id: 'convert', name: '转换格式', desc: 'SRT、VTT、ASS 互相转换（ASS 为默认样式，原有样式会丢失）。' },
  { id: 'danmaku', name: '弹幕转 ASS', desc: '把 B站弹幕 XML 转成 ASS，样式用“设置”里的弹幕样式。' },
]
const op = ref('shift')
const cur = computed(() => OPS.find((o) => o.id === op.value)!)
const files = ref<string[]>([])
const second = ref<string[]>([])
const format = ref('')
const busy = ref(false)
const result = ref<{ output: string; count: number } | null>(null)
const p = reactive({ offset: '-1.0', factor: 1.0427, aFrom: '0:10', aTo: '0:10', bFrom: '1:00:00', bTo: '1:00:00', hearing: true, w: 1920, h: 1080 })

const exts = computed(() => (op.value === 'danmaku' ? ['xml'] : ['srt', 'vtt', 'ass', 'ssa']))

function t(label: string, s: string): number | null {
  const v = parseClock(s.replace(/^-/, ''))
  if (v === null) {
    ElMessage.warning(`${label}的写法不对，请用 83、1:23 或 1:02:03.5 这样的格式。`)
    return null
  }
  return v
}

function build(): SubOp | null {
  switch (op.value) {
    case 'shift': {
      const v = t('偏移', p.offset)
      return v === null ? null : { op: 'shift', offsetMs: p.offset.trim().startsWith('-') ? -v : v }
    }
    case 'rescale':
      return { op: 'rescale', factor: p.factor }
    case 'align': {
      const [a1, a2, b1, b2] = [t('第一个时间点', p.aFrom), t('第一个目标', p.aTo), t('第二个时间点', p.bFrom), t('第二个目标', p.bTo)]
      if ([a1, a2, b1, b2].some((v) => v === null)) return null
      return { op: 'align', aFrom: a1!, aTo: a2!, bFrom: b1!, bTo: b2! }
    }
    case 'merge':
      if (!second.value.length) return ElMessage.warning('请选择第二个字幕文件。'), null
      return { op: 'merge', second: second.value[0] }
    case 'clean':
      return { op: 'clean', dropHearing: p.hearing }
    case 'convert':
      return { op: 'convert' }
    case 'danmaku':
      return { op: 'danmaku', width: p.w, height: p.h }
  }
  return null
}

async function run() {
  if (!files.value.length) return ElMessage.warning('请先选择字幕文件。')
  const o = build()
  if (!o) return
  busy.value = true
  result.value = null
  try {
    result.value = await api.subtitleTool({ ...o, input: files.value[0], format: format.value || null } as SubJob)
    ElMessage.success('已生成新的字幕文件，原文件不会被修改。')
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="st">
    <nav class="ops" aria-label="字幕工具">
      <button v-for="o in OPS" :key="o.id" type="button" class="op" :class="{ on: op === o.id }" @click="op = o.id; result = null">{{ o.name }}</button>
    </nav>
    <div class="panel card">
      <h3>{{ cur.name }}</h3>
      <p class="mute desc">{{ cur.desc }}</p>
      <FileInput v-model="files" :extensions="exts" :kinds="['subtitle']" :label="op === 'danmaku' ? '弹幕 XML 文件' : '字幕文件'" />
      <div class="form">
        <label v-if="op === 'shift'">
          <span>偏移（秒）</span>
          <el-input v-model="p.offset" size="small" class="w mono" />
          <small class="mute">负数提前，正数推后，例如 -1.5 表示提前 1.5 秒。</small>
        </label>
        <template v-if="op === 'align'">
          <label><span>第一处：字幕里</span><el-input v-model="p.aFrom" size="small" class="w mono" /></label>
          <label><span>应该是</span><el-input v-model="p.aTo" size="small" class="w mono" /></label>
          <label><span>第二处：字幕里</span><el-input v-model="p.bFrom" size="small" class="w mono" /></label>
          <label><span>应该是</span><el-input v-model="p.bTo" size="small" class="w mono" /></label>
        </template>
        <label v-if="op === 'rescale'">
          <span>缩放比例</span>
          <el-input-number v-model="p.factor" size="small" :min="0.5" :max="2" :step="0.001" :precision="4" />
          <small class="mute">新时长 ÷ 旧时长。23.976 帧字幕配 25 帧视频用 0.9590；25 帧字幕配 23.976 帧视频用 1.0427。</small>
        </label>
        <label v-if="op === 'merge'">
          <span>第二个字幕</span>
          <FileInput v-model="second" :extensions="['srt', 'vtt', 'ass', 'ssa']" :kinds="['subtitle']" label="第二个字幕文件" />
        </label>
        <label v-if="op === 'clean'"><span>去掉说明</span><el-switch v-model="p.hearing" /><small class="mute">[音乐]、（笑）、【掌声】这类方括号里的内容。</small></label>
        <template v-if="op === 'danmaku'">
          <label><span>画面比例</span><el-select v-model="p.w" size="small" class="w" @change="(v: number) => (p.h = v === 1920 ? 1080 : v === 1080 ? 1920 : 1080)"><el-option :value="1920" label="横屏 16:9" /><el-option :value="1080" label="竖屏 9:16" /></el-select></label>
        </template>
        <label v-if="op !== 'danmaku'">
          <span>输出格式</span>
          <el-radio-group v-model="format" size="small">
            <el-radio-button value="">自动</el-radio-button>
            <el-radio-button value="srt">SRT</el-radio-button>
            <el-radio-button value="vtt">VTT</el-radio-button>
            <el-radio-button value="ass">ASS</el-radio-button>
          </el-radio-group>
        </label>
      </div>
      <div class="go">
        <el-button type="primary" :loading="busy" :disabled="!files.length" @click="run">开始处理</el-button>
        <span v-if="result" class="res">
          已保存为 <span class="mono selectable">{{ result.output.split(/[\\/]/).pop() }}</span><template v-if="result.count">（{{ result.count }} 条）</template>
          <el-button link size="small" type="primary" @click="api.revealFile(result.output)">所在文件夹</el-button>
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.st {
  display: grid;
  grid-template-columns: 150px 1fr;
  gap: 14px;
  align-items: start;
}
.ops {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.op {
  all: unset;
  cursor: pointer;
  padding: 6px 12px;
  border-radius: 7px;
  font-size: 13px;
}
.op:hover,
.op.on {
  background: var(--cc-acc-soft);
}
.op.on {
  color: var(--cc-acc);
  font-weight: 600;
}
.op:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.panel {
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
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
  gap: 10px;
}
.form label {
  display: grid;
  grid-template-columns: 110px auto;
  gap: 10px;
  align-items: center;
  justify-content: start;
}
.form label > span:first-child {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.form label small {
  grid-column: 2;
  font-size: 11.5px;
  max-width: 520px;
}
.w {
  width: 220px;
}
.go {
  display: flex;
  gap: 14px;
  align-items: center;
  flex-wrap: wrap;
}
.res {
  font-size: 12.5px;
}
</style>
