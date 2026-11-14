<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { api } from '../../api'
import type { FtOpts, FtPlan } from '../../types'
import { TARGET_LANGS } from '../../utils/langs'
import { baseName, rangesValid, sizeText } from '../../utils/ft'

/** 开始前确认：文件概况、页范围、语言、输出方式、哪些内容不翻译，以及预计要发多少次请求。 */
const props = defineProps<{ modelValue: boolean; paths: string[]; opts: FtOpts; title?: string; confirmText?: string; modelName?: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: boolean): void; (e: 'confirm', o: FtOpts, remember: boolean): void }>()

const o = ref<FtOpts>({ ...props.opts })
const plan = ref<FtPlan | null>(null)
const busy = ref(false)
const err = ref('')
const askNext = ref(true)
const first = computed(() => props.paths[0] ?? '')
const isPdf = computed(() => /\.pdf$/i.test(first.value))
const rangeOk = computed(() => rangesValid(o.value.pages))
let timer: ReturnType<typeof setTimeout> | null = null
let seq = 0

async function refresh() {
  if (!first.value || !rangeOk.value) return
  const my = ++seq
  busy.value = true
  err.value = ''
  try {
    const p = await api.filetrPlan(first.value, o.value)
    if (my === seq) plan.value = p
  } catch (e) {
    if (my === seq) {
      plan.value = null
      err.value = String(e instanceof Error ? e.message : e)
    }
  } finally {
    if (my === seq) busy.value = false
  }
}
watch(
  () => props.modelValue,
  (v) => {
    if (!v) return
    o.value = { ...props.opts }
    plan.value = null
    askNext.value = true
    void refresh()
  },
  { immediate: true },
)
watch(
  o,
  () => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => void refresh(), 250)
  },
  { deep: true },
)
const close = () => emit('update:modelValue', false)
const summary = computed(() => (plan.value ? sizeText(plan.value.pages, plan.value.segments, plan.value.chars) : busy.value ? '正在读取…' : ''))
const rest = computed(() => (plan.value ? Math.max(0, plan.value.segments - plan.value.cached - plan.value.skipped) : 0))
</script>

<template>
  <el-dialog :model-value="modelValue" :title="title || '开始前确认'" width="520px" append-to-body @update:model-value="(v: boolean) => emit('update:modelValue', v)">
    <div class="sd">
      <div class="file">
        <b :title="first">{{ baseName(first) }}</b>
        <span v-if="paths.length > 1" class="mute">等 {{ paths.length }} 个文件，下面的设置对它们都生效</span>
        <span class="mute">{{ summary }}</span>
        <span v-if="err" class="bad">{{ err }}</span>
      </div>
      <div v-if="isPdf" class="fld">
        <span>页范围</span>
        <el-input v-model="o.pages" size="small" clearable placeholder="全部。如 3-12, 15 或 5-" :class="{ badin: !rangeOk }" />
      </div>
      <div v-if="!rangeOk" class="bad ind">页范围没看懂，写法如 3-12, 15。</div>
      <div class="fld">
        <span>语言</span>
        <el-select v-model="o.target" size="small" filterable allow-create default-first-option class="grow">
          <el-option v-for="l in TARGET_LANGS" :key="l" :value="l" :label="`自动检测 → ${l}`" />
        </el-select>
      </div>
      <div class="fld">
        <span>输出</span>
        <div class="seg">
          <span :class="{ on: o.bilingual }" @click="o.bilingual = true">双语对照</span>
          <span :class="{ on: !o.bilingual }" @click="o.bilingual = false">仅译文</span>
        </div>
      </div>
      <div v-if="isPdf" class="fld">
        <span>PDF</span>
        <div class="seg">
          <span :class="{ on: o.pdfMode === 'inplace' }" @click="o.pdfMode = 'inplace'">原处替换</span>
          <span :class="{ on: o.pdfMode === 'interleave' }" @click="o.pdfMode = 'interleave'">原页+译文页</span>
          <span :class="{ on: o.pdfMode === 'txt' }" @click="o.pdfMode = 'txt'">只要文字</span>
        </div>
      </div>
      <div class="chk">
        <el-checkbox v-model="o.skipHeaders">不翻译页眉、页脚和页码</el-checkbox>
        <el-checkbox v-model="o.skipCode">不翻译公式、代码和网址</el-checkbox>
        <el-checkbox v-model="o.skipRefs">不翻译参考文献</el-checkbox>
        <el-checkbox v-model="o.scanAuto" title="有文字层的 PDF 里夹着扫描页时，自动识字并一起翻译">自动识别扫描页</el-checkbox>
        <el-checkbox v-model="o.preview">先译完预览，我检查后再生成文件</el-checkbox>
      </div>
      <div class="est">
        <template v-if="plan">
          要译 <b>{{ plan.selectedPages }}</b> 页 · <b>{{ rest }}</b> 段<span v-if="plan.cached">（已有缓存 {{ plan.cached }} 段，不重翻）</span><span v-if="plan.skipped">，跳过 {{ plan.skipped }} 段</span>
          <br />预计约 <b>{{ plan.requests }}</b> 次请求<span class="mute">（估算，实际随内容长短浮动）</span>
          <br v-if="plan.scanned" /><span v-if="plan.scanned" class="warn">这份 PDF 有扫描页，要先识字，请求会更多。</span>
        </template>
        <span v-else class="mute">{{ busy ? '正在估算…' : '' }}</span>
        <div class="mute">{{ modelName ? `翻译用 AI 模型池（${modelName}），在“设置 → AI”里换。` : '翻译用 AI 模型池，在“设置 → AI”里换。' }}</div>
      </div>
    </div>
    <template #footer>
      <el-checkbox v-model="askNext" class="ask">以后开始前也确认</el-checkbox>
      <el-button @click="close">取消</el-button>
      <el-button type="primary" :disabled="!rangeOk" @click="emit('confirm', { ...o }, askNext)">{{ confirmText || '开始翻译' }}</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.sd { display: flex; flex-direction: column; gap: 12px }
.file { display: flex; flex-direction: column; gap: 2px; padding: 8px 10px; border-radius: 9px; background: var(--cc-fill) }
.file b { font-size: 14px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mute { color: var(--cc-mute); font-size: 12.5px }
.bad { color: var(--cc-err); font-size: 12.5px }
.warn { color: var(--cc-amber); font-size: 12.5px }
.ind { margin-top: -6px; padding-left: 66px }
.fld { display: flex; align-items: center; gap: 8px }
.fld > span { width: 58px; color: var(--cc-mute); font-size: 12.5px; flex: none }
.grow { flex: 1 }
.seg { display: flex; flex: 1; background: var(--cc-inset); border: 1px solid var(--cc-line); border-radius: 8px; padding: 2px; gap: 2px }
.seg span { flex: 1; text-align: center; padding: 3px 0; border-radius: 6px; color: var(--cc-mute); font-size: 13px; cursor: pointer }
.seg .on { background: var(--cc-amber); color: var(--cc-on-acc); font-weight: 700 }
.chk { display: flex; flex-direction: column; gap: 0 }
.est { font-size: 13px; line-height: 1.7; padding: 8px 10px; border-radius: 9px; border: 1px solid var(--cc-line) }
.ask { margin-right: auto; float: left }
</style>
