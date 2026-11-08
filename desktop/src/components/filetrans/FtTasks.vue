<script setup lang="ts">
import { computed } from 'vue'
import { baseName, filterTasks, primaryAction, STATE_LABEL, taskCounts, type Task, type TaskFilter } from '../../utils/ft'

/** 任务卡片：每种状态只放最常用的一两个按钮，其余收进“⋯”。 */
const props = defineProps<{ tasks: Task[]; cur: string; filter: TaskFilter; eta: Record<string, string> }>()
const emit = defineEmits<{
  (e: 'update:filter', f: TaskFilter): void
  (e: 'pick', t: Task): void
  (e: 'act', what: string, t: Task): void
  (e: 'bulk', what: 'clear' | 'stopAll' | 'resumeAll'): void
}>()

const counts = computed(() => taskCounts(props.tasks))
const shown = computed(() => filterTasks(props.tasks, props.filter))
const FILTERS: [TaskFilter, string][] = [['all', '全部'], ['active', '进行中'], ['done', '已完成'], ['failed', '失败']]
const canClear = computed(() => props.tasks.some((t) => t.state !== 'running' && t.state !== 'queued'))
const anyRunning = computed(() => props.tasks.some((t) => t.state === 'running' || t.state === 'queued'))
const anyStopped = computed(() => props.tasks.some((t) => t.state === 'stopped'))

function tagText(t: Task): string {
  if (t.state === 'running') return t.pagesTotal ? `译第 ${Math.min(t.pagesDone + 1, t.pagesTotal)} / ${t.pagesTotal} 页` : `${Math.round(t.percent)}%`
  if (t.state === 'stopped' && t.pagesTotal) return `已停止 · ${t.pagesDone} / ${t.pagesTotal} 页`
  return STATE_LABEL[t.state]
}
function subText(t: Task): string {
  if (t.missing) return '找不到源文件，可以重新选择文件'
  if (t.state === 'failed') return t.error || '失败了，点“重试失败的”'
  if (t.state === 'running') return [t.note, props.eta[t.key]].filter(Boolean).join(' · ')
  if (t.state === 'stopped') return t.pagesDone ? `前 ${t.pagesDone} 页的译文已保留` : '已停止'
  if (t.state === 'queued') return '前面的任务跑完会自动开始'
  if (t.state === 'ready') return t.failed ? `${t.failed} 段没译出来 · 在右边检查、改译文` : `${t.pagesTotal || ''} 页 · 在右边检查、改译文`
  return t.pagesTotal ? `${t.pagesTotal} 页` : ''
}
const LABEL: Record<string, string> = { cancel: '取消', stop: '停止', resume: '继续', retry: '重试失败的', generate: '生成文件', open: '打开' }
const isPri = (a: string) => a === 'resume' || a === 'retry' || a === 'generate'
const live = (t: Task) => t.state === 'running' || t.state === 'queued'
</script>

<template>
  <div class="card tasks">
    <div class="hd">
      <h3>任务</h3>
      <span class="sp" />
      <el-dropdown trigger="click" @command="(c: string) => emit('bulk', c as never)">
        <el-button size="small" text aria-label="批量操作" :disabled="!tasks.length">批量 ▾</el-button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="clear" :disabled="!canClear">清除已结束的</el-dropdown-item>
            <el-dropdown-item command="stopAll" :disabled="!anyRunning">全部停止</el-dropdown-item>
            <el-dropdown-item command="resumeAll" :disabled="!anyStopped">全部继续</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
    <div v-if="tasks.length" class="flt" role="tablist">
      <span v-for="[k, n] in FILTERS" :key="k" role="tab" :class="{ on: filter === k }" @click="emit('update:filter', k)">{{ n }} {{ counts[k] }}</span>
    </div>
    <div v-if="!tasks.length" class="mute">还没有任务。拖入文件就会开始翻译，原文件不会被修改。</div>
    <div v-else-if="!shown.length" class="mute">这一类里没有任务。</div>
    <div v-for="t in shown" :key="t.key" class="job" :class="{ cur: t.path === cur }" :data-state="t.state" @click="emit('pick', t)">
      <div class="n">
        <b :title="t.path">{{ t.title || baseName(t.path) }}</b>
        <span class="tag" :class="t.state">{{ tagText(t) }}</span>
      </div>
      <div v-if="t.state === 'running' || t.state === 'queued'" class="bar"><i :style="{ width: t.percent + '%' }" /></div>
      <div class="r2">
        <span class="sub" :class="{ bad: t.state === 'failed' || t.missing }" :title="subText(t)">{{ subText(t) }}</span>
        <template v-if="t.missing && !live(t)">
          <el-button size="small" @click.stop="emit('act', 'repick', t)">重新选择文件</el-button>
        </template>
        <template v-else>
          <el-button v-if="t.state === 'queued'" size="small" @click.stop="emit('act', 'cancel', t)">取消</el-button>
          <el-button v-else-if="primaryAction(t) !== 'none'" size="small" :type="isPri(primaryAction(t)) ? 'primary' : undefined" @click.stop="emit('act', primaryAction(t), t)">{{ LABEL[primaryAction(t)] }}</el-button>
          <el-button v-if="t.state === 'generated' && t.output" size="small" @click.stop="emit('act', 'reveal', t)">所在文件夹</el-button>
        </template>
        <el-dropdown v-if="t.state !== 'queued'" trigger="click" @command="(c: string) => emit('act', c, t)" @click.stop>
          <el-button size="small" aria-label="更多" @click.stop>⋯</el-button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="read">在阅读器里打开</el-dropdown-item>
              <el-dropdown-item command="source" :disabled="t.missing">打开原文件所在位置</el-dropdown-item>
              <el-dropdown-item command="regen" divided :disabled="live(t) || t.missing">换设置重新生成…</el-dropdown-item>
              <el-dropdown-item command="retranslate" :disabled="live(t) || t.missing">重新翻译（清掉这份译文）</el-dropdown-item>
              <el-dropdown-item command="copyerr" divided :disabled="!t.error">复制出错信息</el-dropdown-item>
              <el-dropdown-item command="remove" :disabled="live(t)">从列表里移除</el-dropdown-item>
              <el-dropdown-item command="delcache" :disabled="live(t)"><span class="dng">删除译文缓存…</span></el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
    </div>
  </div>
</template>

<style scoped>
.card { border: 1px solid var(--el-border-color); background: var(--el-bg-color); border-radius: 12px; padding: 12px 14px; display: flex; flex-direction: column; gap: 8px }
.hd { display: flex; align-items: center; gap: 6px }
.hd h3 { margin: 0; font-size: 13px; color: var(--el-text-color-secondary); font-weight: 600 }
.sp { flex: 1 }
.mute { color: var(--el-text-color-secondary); font-size: 12.5px }
.flt { display: flex; gap: 4px; flex-wrap: wrap }
.flt span { font-size: 12px; padding: 1px 9px; border-radius: 999px; border: 1px solid var(--el-border-color); cursor: pointer; color: var(--el-text-color-secondary) }
.flt .on { background: var(--el-color-primary-light-9); border-color: var(--el-color-primary-light-5); color: var(--el-color-primary); font-weight: 700 }
.job { display: flex; flex-direction: column; gap: 5px; padding: 8px 10px; border-radius: 9px; border: 1px solid var(--el-border-color); cursor: pointer }
.job.cur { border-color: var(--el-color-primary); background: var(--el-color-primary-light-9) }
.n { display: flex; gap: 8px; align-items: center }
.n b { flex: 1; min-width: 0; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.tag { font-size: 11.5px; border-radius: 999px; padding: 0 8px; line-height: 20px; white-space: nowrap; background: var(--el-fill-color); color: var(--el-text-color-regular) }
.tag.running { background: var(--el-color-primary-light-9); color: var(--el-color-primary) }
.tag.failed { background: var(--el-color-danger-light-9); color: var(--el-color-danger) }
.tag.ready { background: var(--el-color-warning-light-9); color: var(--el-color-warning-dark-2) }
.tag.generated { background: var(--el-color-success-light-9); color: var(--el-color-success) }
.bar { height: 5px; border-radius: 3px; background: var(--el-border-color); overflow: hidden }
.bar i { display: block; height: 100%; background: var(--el-color-primary); transition: width 0.3s }
.r2 { display: flex; align-items: center; gap: 6px }
.sub { flex: 1; min-width: 0; font-size: 12px; color: var(--el-text-color-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.sub.bad { color: var(--el-color-danger) }
.dng { color: var(--el-color-danger) }
</style>
