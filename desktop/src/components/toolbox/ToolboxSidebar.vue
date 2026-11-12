<script setup lang="ts">
import { computed, ref } from 'vue'
import { GROUPS, TOOLS, byId, matches, type ToolDef } from '../../utils/toolbox'

const props = defineProps<{ modelValue: string; recent: string[] }>()
const emit = defineEmits<{ 'update:modelValue': [id: string]; back: [] }>()

const query = ref('')
const sections = computed(() =>
  GROUPS.map((g) => ({ ...g, tools: TOOLS.filter((t) => t.group === g.id && matches(t, query.value)) })).filter((g) => g.tools.length),
)
const recentTools = computed<ToolDef[]>(() => (query.value.trim() ? [] : props.recent.map(byId)))

function pickFirst() {
  const first = sections.value[0]?.tools[0]
  if (first) emit('update:modelValue', first.id)
}
</script>

<template>
  <nav class="tbs" aria-label="工具分组">
    <div class="top">
      <button type="button" class="back" aria-label="返回上一页" title="返回进工具箱之前的页面" @click="emit('back')">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 5l-7 7 7 7" /></svg>
      </button>
      <label class="search">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></svg>
        <input v-model="query" aria-label="搜索工具" placeholder="搜索工具…" @keydown.enter="pickFirst" />
      </label>
    </div>
    <div v-if="recentTools.length" class="sec">
      <div class="head"><span class="dot muted" />最近用过</div>
      <button v-for="t in recentTools" :key="'r-' + t.id" type="button" class="item recent" @click="emit('update:modelValue', t.id)">{{ t.name }}</button>
    </div>
    <div v-for="g in sections" :key="g.id" class="sec">
      <div class="head"><span class="dot" :style="{ background: g.color }" />{{ g.name }}</div>
      <button v-for="t in g.tools" :key="t.id" type="button" class="item" :class="{ on: modelValue === t.id }" :aria-current="modelValue === t.id ? 'true' : undefined" @click="emit('update:modelValue', t.id)">
        <span>{{ t.name }}</span>
        <small v-if="t.tag">{{ t.tag }}</small>
      </button>
    </div>
    <p v-if="!sections.length" class="empty mute">没有找到“{{ query }}”相关的工具。</p>
  </nav>
</template>

<style scoped>
.tbs {
  width: 230px;
  flex: none;
  box-sizing: border-box;
  padding: 22px 12px 14px;
  border-right: 1px solid var(--cc-line);
  background: var(--cc-rail);
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
}
.top {
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
}
.back {
  all: unset;
  box-sizing: border-box;
  width: 34px;
  height: 34px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--cc-line-strong);
  border-radius: 10px;
  background: var(--cc-inset);
  color: var(--cc-fg);
  cursor: pointer;
}
.back:hover {
  background: var(--cc-acc-soft);
}
.back:focus-visible,
.item:focus-visible {
  outline: 2px solid var(--cc-acc);
}
.search {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 34px;
  box-sizing: border-box;
  padding: 0 10px;
  border: 1px solid var(--cc-line-strong);
  border-radius: 10px;
  background: var(--cc-inset);
  color: var(--cc-mute);
}
.search input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: 0;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--cc-fg);
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 10px 4px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  color: var(--cc-mute);
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 3px;
}
.dot.muted {
  background: var(--cc-line);
}
.item {
  all: unset;
  box-sizing: border-box;
  width: 100%;
  height: 36px;
  padding: 0 12px;
  border-radius: 10px;
  border: 1px solid transparent;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  font-size: 13px;
  cursor: pointer;
}
.item:hover {
  background: var(--cc-panel);
}
.item.on {
  background: var(--cc-panel);
  border-color: var(--cc-line-strong);
  box-shadow: inset 2px 0 0 var(--cc-amber);
  color: var(--cc-amber);
  font-weight: 600;
}
.item.recent {
  color: var(--cc-mute);
}
.item small {
  font-size: 11px;
  color: var(--cc-mute);
  font-weight: 400;
}
.item.on small {
  color: var(--cc-acc);
}
.empty {
  padding: 12px 10px;
  font-size: 12.5px;
}
</style>
