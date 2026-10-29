<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api, errorText } from '../api'
import type { SiteRule } from '../types'

const rules = defineModel<SiteRule[]>({ required: true })
const testing = ref<number | null>(null)
const testUrl = ref<Record<number, string>>({})
const result = ref<Record<number, string>>({})

function add() {
  rules.value.push({ name: '新规则', enabled: true, pattern: '', videoRegex: '', titleRegex: '', coverRegex: '', referer: '', userAgent: '' })
}

async function test(i: number) {
  const url = (testUrl.value[i] ?? '').trim()
  if (!url) return ElMessage.warning('请先填写一个要测试的网页地址。')
  testing.value = i
  try {
    const info = await api.siteRuleTest(rules.value[i], url)
    const vids = info.assets.filter((a) => a.kind === 'video')
    result.value[i] = `✓ 标题：${info.title}；找到 ${vids.length} 个视频地址，第一个：${vids[0]?.url ?? '—'}`
  } catch (e) {
    result.value[i] = `✗ ${errorText(e)}`
  } finally {
    testing.value = null
  }
}
</script>

<template>
  <section class="group card">
    <h3>自定义站点规则</h3>
    <small class="mute">内置解析器和 yt-dlp 都不支持的网站，可以自己写一条正则从网页源码里取视频地址。规则优先于其他解析方式。需要懂一点正则；先用“测试”确认能取到。</small>
    <div v-for="(r, i) in rules" :key="i" class="rule">
      <div class="top">
        <el-switch v-model="r.enabled" size="small" />
        <el-input v-model="r.name" size="small" placeholder="规则名称（显示为平台名）" class="name" />
        <el-button link size="small" @click="rules.splice(i, 1)">删除</el-button>
      </div>
      <div class="grid">
        <label>网址包含</label>
        <el-input v-model="r.pattern" size="small" class="mono" placeholder="example.com/play/　（以 re: 开头表示正则）" />
        <label>视频地址正则</label>
        <el-input v-model="r.videoRegex" size="small" class="mono" placeholder='data-src="([^"]+\.m3u8[^"]*)"　（取第一个括号里的内容）' />
        <label>标题正则</label>
        <el-input v-model="r.titleRegex" size="small" class="mono" placeholder='<h1 class="t">(.*?)</h1>　（可选，留空用网页标题）' />
        <label>封面正则</label>
        <el-input v-model="r.coverRegex" size="small" class="mono" placeholder="可选，留空用 og:image" />
        <label>Referer</label>
        <el-input v-model="r.referer" size="small" class="mono" placeholder="可选，留空用网页地址" />
        <label>User-Agent</label>
        <el-input v-model="r.userAgent" size="small" class="mono" placeholder="可选，留空用电脑浏览器的" />
      </div>
      <div class="test">
        <el-input v-model="testUrl[i]" size="small" placeholder="粘贴一个要测试的网页地址" />
        <el-button size="small" :loading="testing === i" @click="test(i)">测试</el-button>
      </div>
      <div v-if="result[i]" class="res selectable" :class="{ bad: result[i].startsWith('✗') }">{{ result[i] }}</div>
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
  gap: 10px;
  align-items: center;
}
.name {
  flex: 1;
}
.grid {
  display: grid;
  grid-template-columns: 96px 1fr;
  gap: 6px 10px;
  align-items: center;
}
.grid label {
  font-size: 12px;
  color: var(--cc-mute);
}
.test {
  display: flex;
  gap: 8px;
}
.res {
  font-size: 12px;
  color: var(--cc-ok);
  word-break: break-all;
}
.res.bad {
  color: var(--cc-err);
}
</style>
