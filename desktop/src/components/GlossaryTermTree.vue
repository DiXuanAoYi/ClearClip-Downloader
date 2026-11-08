<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { api } from '../api'
import type { GlossaryTerm } from '../types'
import VirtualList from './VirtualList.vue'
import * as T from '../utils/termtree'

/**
 * 分层术语的编辑区：工具条、表头、树形术语（每一层一种底色）、右键 / ⋯ 菜单、拖动整理、删除确认。
 * 编辑时用“行”（见 utils/termtree），保存时由上层调用 `tree()` 取回树。
 */
const props = defineProps<{ terms: GlossaryTerm[]; canImport?: boolean }>()
const emit = defineEmits<{ path: [src: string[], tgt: string[]]; importCsv: [] }>()

const rows = ref<T.Row[]>(T.toRows(props.terms))
const collapsed = ref<Set<number>>(new Set())
const activeId = ref<number | null>(null)
const query = ref('')
const vlist = ref<{ scrollToIndex: (i: number) => void } | null>(null)
const ROW_H = 38
const GAP = 3

defineExpose({ tree: () => T.toTree(rows.value.filter((r) => r.source.trim() || hasKidsAt(rows.value.indexOf(r)))) })

// ---------- 撤回（结构改动：删除、移动、升降层、拖动） ----------
const undoStack = ref<string[]>([])
function snap() {
  undoStack.value.push(JSON.stringify(rows.value))
  if (undoStack.value.length > 50) undoStack.value.shift()
}
function undo() {
  const s = undoStack.value.pop()
  if (!s) return
  rows.value = JSON.parse(s)
}

// ---------- 派生数据 ----------
/** 每行连同下级到哪里（不含） */
const ends = computed(() => {
  const rs = rows.value
  const end = new Array<number>(rs.length).fill(rs.length)
  const stack: number[] = []
  rs.forEach((r, i) => {
    while (stack.length && rs[stack[stack.length - 1]].level >= r.level) end[stack.pop()!] = i
    stack.push(i)
  })
  return end
})
const hasKidsAt = (i: number) => i >= 0 && ends.value[i] > i + 1
const search = computed(() => T.searchRows(rows.value, query.value))
interface View {
  r: T.Row
  i: number
  kids: number
  open: boolean
  dim: boolean
}
const visible = computed<View[]>(() => {
  const rs = rows.value
  const out: View[] = []
  const q = query.value.trim()
  let hideBelow = Infinity
  rs.forEach((r, i) => {
    const kids = ends.value[i] - i - 1
    if (q) {
      if (search.value.shown.has(i)) out.push({ r, i, kids, open: true, dim: !search.value.hit.has(i) })
      return
    }
    if (r.level > hideBelow) return
    hideBelow = Infinity
    const open = !collapsed.value.has(r.id)
    out.push({ r, i, kids, open, dim: false })
    if (kids && !open) hideBelow = r.level
  })
  return out
})
const total = computed(() => rows.value.filter((r) => r.source.trim()).length)
const depth = computed(() => T.depthOf(rows.value))
const activeIndex = computed(() => rows.value.findIndex((r) => r.id === activeId.value))

watch(
  [activeIndex, rows],
  () => {
    const i = activeIndex.value
    if (i < 0) return emit('path', [], [])
    const p = T.pathOf(rows.value, i).map((j) => rows.value[j])
    emit(
      'path',
      p.map((r) => r.source || '（未填）'),
      p.map((r) => r.target || r.source || '（未填）'),
    )
  },
  { deep: true, immediate: true },
)

// ---------- 焦点 ----------
async function focusRow(id: number, col: 'source' | 'target' | 'note' = 'source') {
  activeId.value = id
  // 收起的上级要展开，搜索时新行看不到就清掉搜索
  const i = rows.value.findIndex((r) => r.id === id)
  for (const j of T.pathOf(rows.value, i).slice(0, -1)) collapsed.value.delete(rows.value[j].id)
  if (query.value && !search.value.shown.has(i)) query.value = ''
  await nextTick()
  const vi = visible.value.findIndex((v) => v.r.id === id)
  if (vi >= 0) vlist.value?.scrollToIndex(vi)
  await nextTick()
  await new Promise((r) => requestAnimationFrame(r))
  const el = document.querySelector(`.tt [data-rid="${id}"] input[data-col="${col}"]`) as HTMLInputElement | null
  el?.focus()
}

// ---------- 操作 ----------
function addTerm() {
  const i = activeIndex.value
  const at = T.addSibling(rows.value, i)
  focusRow(rows.value[at].id)
}
function addSiblingOf(i: number) {
  const at = T.addSibling(rows.value, i)
  focusRow(rows.value[at].id)
}
function addChildOf(i = activeIndex.value) {
  if (i < 0) return ElMessage.info('先点选一条术语，再给它加下级。')
  const at = T.addChild(rows.value, i)
  if (at < 0) return ElMessage.warning(`最多 ${T.MAX_DEPTH} 层。`)
  collapsed.value.delete(rows.value[i].id)
  focusRow(rows.value[at].id)
}
function doIndent(i: number) {
  const id = rows.value[i].id
  snap()
  if (!T.indent(rows.value, i)) {
    undoStack.value.pop()
    return ElMessage.info(i === 0 || rows.value[i - 1].level < rows.value[i].level ? '上面没有同级的术语，不能变成下级。' : `最多 ${T.MAX_DEPTH} 层。`)
  }
  focusRow(id)
}
function doOutdent(i: number) {
  const id = rows.value[i].id
  snap()
  if (T.outdent(rows.value, i) < 0) {
    undoStack.value.pop()
    return
  }
  focusRow(id)
}
function doMove(i: number, dir: -1 | 1) {
  const id = rows.value[i].id
  snap()
  if (T.move(rows.value, i, dir) < 0) undoStack.value.pop()
  else focusRow(id)
}
function toggleOpen(r: T.Row, open?: boolean) {
  const s = new Set(collapsed.value)
  const want = open ?? s.has(r.id)
  if (want) s.delete(r.id)
  else s.add(r.id)
  collapsed.value = s
}
function expandAll(on: boolean) {
  collapsed.value = on ? new Set() : new Set(rows.value.filter((_, i) => hasKidsAt(i)).map((r) => r.id))
}

// 删除：没有下级的直接删；有下级的问一次
const confirmDel = ref<{ i: number; r: T.Row; kids: string[]; n: number } | null>(null)
function askRemove(i: number) {
  const n = ends.value[i] - i - 1
  if (!n) return doRemove(i, true)
  const kids = rows.value.slice(i + 1, ends.value[i]).filter((r) => r.level === rows.value[i].level + 1).map((r) => r.source || '（未填）')
  confirmDel.value = { i, r: rows.value[i], kids, n }
}
function doRemove(i: number, withKids: boolean) {
  snap()
  const prev = rows.value[i - 1]?.id ?? null
  T.remove(rows.value, i, withKids)
  confirmDel.value = null
  activeId.value = prev
}
async function copyPath(i: number) {
  const p = T.pathOf(rows.value, i).map((j) => rows.value[j])
  const text = `${p.map((r) => r.source).join(' › ')} = ${p.map((r) => r.target || r.source).join(' › ')}`
  try {
    await api.copyText(text)
    ElMessage.success('已复制路径')
  } catch {
    navigator.clipboard?.writeText(text).catch(() => {})
  }
}

// ---------- 菜单（⋯ 或右键） ----------
const menu = ref<{ x: number; y: number; i: number } | null>(null)
function openMenu(e: MouseEvent, i: number) {
  e.preventDefault()
  activeId.value = rows.value[i].id
  const w = 280
  const h = 340
  menu.value = { x: Math.min(e.clientX, window.innerWidth - w - 8), y: Math.min(e.clientY, window.innerHeight - h - 8), i }
}
function closeMenu() {
  menu.value = null
}
/** 菜单项：先记下是哪一行，再关菜单、执行 */
function act(fn: (i: number) => void) {
  const i = menu.value?.i
  closeMenu()
  if (i !== undefined) fn(i)
}
const onDocDown = (e: MouseEvent) => {
  if (menu.value && !(e.target as HTMLElement).closest('.tt-menu')) closeMenu()
}
const onDocKey = (e: KeyboardEvent) => {
  if (e.key === 'Escape' && menu.value) {
    e.stopPropagation()
    closeMenu()
  }
}
onMounted(() => {
  document.addEventListener('mousedown', onDocDown, true)
  document.addEventListener('keydown', onDocKey, true)
})
onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onDocDown, true)
  document.removeEventListener('keydown', onDocKey, true)
})

// ---------- 键盘 ----------
const COLS = ['source', 'target', 'note'] as const
function onKey(e: KeyboardEvent, v: View, col: (typeof COLS)[number]) {
  const i = v.i
  const r = v.r
  if (e.isComposing) return
  if (e.key === 'Enter' && !e.altKey && !e.ctrlKey && !e.metaKey) {
    e.preventDefault()
    const at = T.addSibling(rows.value, i)
    focusRow(rows.value[at].id)
  } else if (e.key === 'Tab') {
    e.preventDefault()
    if (e.shiftKey) doOutdent(i)
    else doIndent(i)
  } else if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
    e.preventDefault()
    doMove(i, e.key === 'ArrowUp' ? -1 : 1)
  } else if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
    e.preventDefault()
    if (v.kids) toggleOpen(r, e.key === 'ArrowRight')
  } else if (!e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
    const vi = visible.value.findIndex((x) => x.r.id === r.id) + (e.key === 'ArrowUp' ? -1 : 1)
    const to = visible.value[vi]
    if (to) {
      e.preventDefault()
      focusRow(to.r.id, col)
    }
  } else if (e.key === 'Backspace' && col === 'source' && !r.source && !r.target && !r.note && !v.kids) {
    // 空行上退格：删掉这一行，回到上一行
    e.preventDefault()
    const prev = visible.value[visible.value.findIndex((x) => x.r.id === r.id) - 1]
    rows.value.splice(i, 1)
    if (prev) focusRow(prev.r.id)
  } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && undoStack.value.length && !(e.target as HTMLInputElement).value) {
    e.preventDefault()
    undo()
  }
}
function onAreaKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && (e.target as HTMLElement).tagName !== 'INPUT' && undoStack.value.length) {
    e.preventDefault()
    undo()
  }
}

// ---------- 拖动 ----------
const dragId = ref<number | null>(null)
const dropAt = ref<{ id: number; where: 'before' | 'after' | 'inside' } | null>(null)
function onDragStart(e: DragEvent, r: T.Row) {
  dragId.value = r.id
  activeId.value = r.id
  e.dataTransfer?.setData('text/plain', r.source)
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}
function onDragOver(e: DragEvent, v: View) {
  if (dragId.value === null) return
  const from = rows.value.findIndex((r) => r.id === dragId.value)
  if (v.i >= from && v.i < ends.value[from]) {
    dropAt.value = null
    return
  }
  e.preventDefault()
  const box = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const y = (e.clientY - box.top) / box.height
  dropAt.value = { id: v.r.id, where: y < 0.28 ? 'before' : y > 0.72 ? 'after' : 'inside' }
}
function onDrop(e: DragEvent, v: View) {
  e.preventDefault()
  const d = dropAt.value
  const from = rows.value.findIndex((r) => r.id === dragId.value)
  dragId.value = null
  dropAt.value = null
  if (!d || from < 0) return
  const id = rows.value[from].id
  snap()
  if (T.drop(rows.value, from, v.i, d.where) < 0) {
    undoStack.value.pop()
    return ElMessage.warning(`放不下：最多 ${T.MAX_DEPTH} 层。`)
  }
  if (d.where === 'inside') collapsed.value.delete(v.r.id)
  focusRow(id)
}
function onDragEnd() {
  dragId.value = null
  dropAt.value = null
}
const dropClass = (v: View) => (dropAt.value?.id === v.r.id ? `drop-${dropAt.value.where}` : '')
</script>

<template>
  <div class="tt" @keydown="onAreaKey">
    <!-- 工具条 -->
    <div class="tbar">
      <el-input v-model="query" clearable placeholder="搜索原文、译文、说明" class="q" aria-label="搜索术语">
        <template #prefix>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></svg>
        </template>
      </el-input>
      <el-button :title="activeId !== null ? '加在当前这条的后面（同级）' : '加在最后'" @click="addTerm">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg><span>添加术语</span>
      </el-button>
      <el-button :disabled="activeIndex < 0" title="给当前这条加一个下级" @click="addChildOf()">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4v9a3 3 0 0 0 3 3h9" /><path d="M15 13l3 3-3 3" /></svg><span>添加下级</span>
      </el-button>
      <span class="sep" />
      <el-button @click="expandAll(true)">全部展开</el-button>
      <el-button @click="expandAll(false)">全部收起</el-button>
      <el-button v-if="canImport" @click="emit('importCsv')">并入 CSV…</el-button>
      <span class="grow" />
      <button v-if="undoStack.length" type="button" class="pz-link" title="Ctrl+Z" @click="undo">撤回</button>
      <span class="cnt">共 <span class="mono">{{ total }}</span> 条 · <span class="mono">{{ depth }}</span> 层</span>
    </div>

    <!-- 表头 -->
    <div class="thead">
      <span />
      <span>原文</span>
      <span>译文</span>
      <span>说明（给模型的提示）</span>
      <span />
    </div>

    <!-- 树 -->
    <div v-if="!visible.length" class="empty pz-note">{{ query ? '没有匹配的术语。' : '还没有术语。点“添加术语”，或者并入一个 CSV。' }}</div>
    <VirtualList v-else ref="vlist" role="tree" aria-label="术语" class="body" :items="visible" :item-height="ROW_H" :gap="GAP" :item-key="(v: View) => v.r.id">
      <template #default="{ item: v }">
        <div
          role="treeitem"
          :aria-level="v.r.level + 1"
          :aria-expanded="v.kids ? v.open : undefined"
          class="trow"
          :class="[`lv${Math.min(v.r.level, 2)}`, dropClass(v), { on: v.r.id === activeId, dim: v.dim, grp: v.r.group, dragging: v.r.id === dragId }]"
          :data-rid="v.r.id"
          @mousedown="activeId = v.r.id"
          @contextmenu="openMenu($event, v.i)"
          @dragover="onDragOver($event, v)"
          @dragleave="dropAt?.id === v.r.id && (dropAt = null)"
          @drop="onDrop($event, v)"
        >
          <span class="grip" draggable="true" title="拖动整理（连同下级一起移动）" @dragstart="onDragStart($event, v.r)" @dragend="onDragEnd">
            <svg width="10" height="14" viewBox="0 0 10 14" aria-hidden="true"><g fill="currentColor"><circle cx="2.5" cy="2.5" r="1.3" /><circle cx="7.5" cy="2.5" r="1.3" /><circle cx="2.5" cy="7" r="1.3" /><circle cx="7.5" cy="7" r="1.3" /><circle cx="2.5" cy="11.5" r="1.3" /><circle cx="7.5" cy="11.5" r="1.3" /></g></svg>
          </span>
          <div class="tcell">
            <span v-for="g in v.r.level" :key="g" class="guide" aria-hidden="true" />
            <button type="button" class="caret" :class="{ open: v.open, none: !v.kids }" :aria-label="v.open ? '收起' : '展开'" :tabindex="v.kids ? 0 : -1" @click="v.kids && toggleOpen(v.r)">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
            </button>
            <input v-model="v.r.source" class="cell src" data-col="source" aria-label="原文" placeholder="原文" @focus="activeId = v.r.id" @keydown="onKey($event, v, 'source')" />
            <span v-if="v.r.group" class="tag">分组</span>
            <span v-if="v.kids && !v.open" class="tag">{{ v.kids }} 条下级</span>
          </div>
          <input v-model="v.r.target" class="cell" data-col="target" aria-label="译文" :placeholder="v.r.group ? '（只作分组）' : '留空 = 保持原文'" @focus="activeId = v.r.id" @keydown="onKey($event, v, 'target')" />
          <input v-model="v.r.note" class="cell note" data-col="note" aria-label="说明" @focus="activeId = v.r.id" @keydown="onKey($event, v, 'note')" />
          <div class="acts">
            <button type="button" class="ib" aria-label="添加下级" title="添加下级（Tab）" @click="addChildOf(v.i)">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4v9a3 3 0 0 0 3 3h9" /><path d="M15 13l3 3-3 3" /></svg>
            </button>
            <button type="button" class="ib" aria-label="更多操作" title="更多操作" @click.stop="openMenu($event, v.i)">
              <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"><g fill="currentColor"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></g></svg>
            </button>
          </div>
        </div>
      </template>
    </VirtualList>

    <!-- ⋯ / 右键菜单 -->
    <Teleport to="body">
      <div v-if="menu" class="tt-menu" role="menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }">
        <button type="button" role="menuitem" class="mi" @click="act(addSiblingOf)">添加同级<span class="kbd">Enter</span></button>
        <button type="button" role="menuitem" class="mi" @click="act((i) => addChildOf(i))">添加下级<span class="kbd">Tab</span></button>
        <div class="msep" />
        <button type="button" role="menuitem" class="mi" :disabled="rows[menu.i].level === 0" @click="act(doOutdent)">升一层<span class="kbd">Shift+Tab</span></button>
        <button type="button" role="menuitem" class="mi" @click="act(doIndent)">放进上一条下面<span class="kbd">Tab</span></button>
        <button type="button" role="menuitem" class="mi" @click="act((i) => doMove(i, -1))">上移<span class="kbd">Alt+↑</span></button>
        <button type="button" role="menuitem" class="mi" @click="act((i) => doMove(i, 1))">下移<span class="kbd">Alt+↓</span></button>
        <div class="msep" />
        <button type="button" role="menuitem" class="mi" @click="act((i) => (rows[i].group = !rows[i].group))">只作分组（不当术语用）<span class="ck">{{ rows[menu.i].group ? '✓' : '' }}</span></button>
        <button type="button" role="menuitem" class="mi" @click="act(copyPath)">复制路径<span class="hint">{{ T.pathOf(rows, menu.i).map((j) => rows[j].source).join(' › ') }}</span></button>
        <button type="button" role="menuitem" class="mi danger" @click="act(askRemove)">删除…<span v-if="hasKidsAt(menu.i)" class="hint">含 {{ ends[menu.i] - menu.i - 1 }} 条下级</span></button>
      </div>
    </Teleport>

    <!-- 删除有下级的术语 -->
    <el-dialog :model-value="!!confirmDel" :title="`删除“${confirmDel?.r.source || '（未填）'}”？`" width="440px" append-to-body align-center @close="confirmDel = null">
      <div v-if="confirmDel" class="del">
        <span class="pz-note">它下面还有 {{ confirmDel.n }} 条术语：{{ confirmDel.kids.slice(0, 5).join('、') }}{{ confirmDel.kids.length > 5 ? '…' : '' }}。</span>
        <button type="button" class="opt" @click="doRemove(confirmDel.i, false)">
          <b>只删“{{ confirmDel.r.source || '（未填）' }}”</b><small>下级升一层，留在原来的位置</small>
        </button>
        <button type="button" class="opt danger" @click="doRemove(confirmDel.i, true)">
          <b>连同下级一起删（{{ confirmDel.n + 1 }} 条）</b><small>保存前可以 Ctrl+Z 撤回</small>
        </button>
      </div>
      <template #footer><el-button @click="confirmDel = null">取消</el-button></template>
    </el-dialog>
  </div>
</template>

<style scoped>
.tt {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
}
.tbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 12px 24px;
  border-top: 1px solid var(--cc-line-soft);
  border-bottom: 1px solid var(--cc-line-soft);
  background: var(--cc-foot);
}
.tbar .el-button {
  margin: 0;
  height: 32px;
}
.tbar .el-button svg {
  margin-right: 6px;
}
.q {
  width: 220px;
}
.sep {
  width: 1px;
  height: 20px;
  background: var(--cc-line);
  margin: 0 4px;
}
.grow {
  flex: 1;
}
.cnt {
  font-size: 12px;
  color: var(--cc-mute);
}
.thead,
.trow {
  display: grid;
  grid-template-columns: 24px minmax(0, 1.25fr) minmax(0, 1fr) minmax(0, 0.8fr) 76px;
  gap: 8px;
  align-items: center;
}
.thead {
  height: 34px;
  padding: 0 24px 0 24px;
  font-size: 12px;
  color: var(--cc-mute);
  border-bottom: 1px solid var(--cc-line-soft);
}
.body {
  flex: 1;
  min-height: 160px;
  padding: 6px 12px;
}
.empty {
  padding: 24px;
}
.trow {
  height: 38px;
  padding: 0 12px 0 0;
  border-radius: 6px;
  box-sizing: border-box;
  position: relative;
}
.trow.lv0 {
  background: var(--cc-lv0);
  box-shadow: inset 0 0 0 1px var(--cc-lvline0);
}
.trow.lv1 {
  background: var(--cc-lv1);
  box-shadow: inset 0 0 0 1px var(--cc-lvline1);
}
.trow.lv2 {
  background: var(--cc-lv2);
  box-shadow: inset 0 0 0 1px var(--cc-lvline2);
}
.trow.on {
  background: var(--cc-acc-soft);
  box-shadow: inset 0 0 0 1px var(--cc-acc-line);
}
.trow.dim .cell {
  color: var(--cc-mute);
}
.trow.dragging {
  opacity: 0.4;
}
.trow.drop-inside {
  box-shadow: inset 0 0 0 2px var(--cc-acc);
}
.trow.drop-before::before,
.trow.drop-after::after {
  content: '';
  position: absolute;
  left: 24px;
  right: 0;
  height: 2px;
  background: var(--cc-acc);
  border-radius: 1px;
}
.trow.drop-before::before {
  top: -3px;
}
.trow.drop-after::after {
  bottom: -3px;
}
.grip {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--cc-dot);
  cursor: grab;
  visibility: hidden;
}
.trow:hover .grip,
.trow.on .grip {
  visibility: visible;
}
.tcell {
  display: flex;
  align-items: center;
  min-width: 0;
  height: 100%;
}
.guide {
  flex: none;
  width: 20px;
  align-self: stretch;
  margin-left: 9px;
  border-left: 1px solid var(--cc-guide);
  box-sizing: border-box;
}
.caret {
  flex: none;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--cc-mute);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.caret svg {
  transition: transform 0.12s;
}
.caret.open svg {
  transform: rotate(90deg);
}
.caret.none {
  visibility: hidden;
}
.cell {
  height: 30px;
  min-width: 0;
  width: 100%;
  box-sizing: border-box;
  padding: 0 8px;
  border: 1px solid transparent;
  border-radius: 5px;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--cc-fg);
  outline: none;
}
.cell:hover {
  border-color: var(--cc-line);
}
.cell:focus {
  background: var(--cc-card);
  border-color: var(--cc-acc);
}
.cell::placeholder {
  color: var(--cc-mute);
  opacity: 0.7;
}
.lv0 .src,
.lv0 .cell:not(.note) {
  font-weight: 600;
}
.grp .src {
  color: var(--cc-fg2);
}
.note {
  color: var(--cc-mute);
  font-size: 12.5px;
}
.tag {
  flex: none;
  margin-left: 4px;
  display: inline-flex;
  align-items: center;
  height: 18px;
  padding: 0 6px;
  border-radius: 4px;
  background: var(--cc-fill);
  color: var(--cc-fg2);
  font-size: 11px;
  font-weight: 400;
  white-space: nowrap;
}
.acts {
  display: flex;
  justify-content: flex-end;
  gap: 2px;
  visibility: hidden;
}
.trow:hover .acts,
.trow.on .acts {
  visibility: visible;
}
.ib {
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--cc-mute);
  cursor: pointer;
}
.ib:hover {
  background: var(--cc-hover);
  color: var(--cc-fg);
}
.del {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.opt {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 14px;
  border: 1px solid var(--cc-line);
  border-radius: 6px;
  background: var(--cc-card);
  font: inherit;
  color: var(--cc-fg);
  text-align: left;
  cursor: pointer;
}
.opt:hover {
  border-color: var(--cc-dot);
}
.opt b {
  font-weight: 600;
}
.opt small {
  font-size: 12px;
  color: var(--cc-mute);
}
.opt.danger {
  border-color: var(--cc-err);
}
.opt.danger b {
  color: var(--cc-err);
}
</style>

<style>
/* 菜单挂在 body 上，不能用 scoped */
.tt-menu {
  position: fixed;
  z-index: 3000;
  width: 280px;
  padding: 6px;
  background: var(--cc-card);
  border: 1px solid var(--cc-line);
  border-radius: 10px;
  box-shadow: 0 12px 32px #0000002e;
  font-family: var(--cc-font);
}
.tt-menu .mi {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  width: 100%;
  height: 32px;
  padding: 0 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--cc-fg);
  text-align: left;
  cursor: pointer;
}
.tt-menu .mi:hover:not(:disabled) {
  background: var(--cc-hover);
}
.tt-menu .mi:disabled {
  color: var(--cc-mute);
  cursor: default;
}
.tt-menu .mi.danger {
  color: var(--cc-err);
}
.tt-menu .kbd {
  display: inline-flex;
  align-items: center;
  height: 18px;
  padding: 0 5px;
  border: 1px solid var(--cc-line);
  border-radius: 4px;
  background: var(--cc-fill);
  font-family: var(--cc-mono);
  font-size: 11px;
  color: var(--cc-fg2);
}
.tt-menu .hint {
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11.5px;
  color: var(--cc-mute);
}
.tt-menu .mi.danger .hint {
  color: var(--cc-err);
}
.tt-menu .ck {
  color: var(--cc-acc-text);
  font-weight: 600;
}
.tt-menu .msep {
  height: 1px;
  background: var(--cc-line-soft);
  margin: 4px 6px;
}
</style>
