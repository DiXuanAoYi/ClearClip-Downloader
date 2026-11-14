<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { errorText } from '../api'
import { useAppStore } from '../stores/app'
import type { Settings } from '../types'
import { clone, merge3 } from '../utils/merge3'
import AccountsPanel from '../components/AccountsPanel.vue'
import NetworkPanel from '../components/NetworkPanel.vue'
import ComponentsPanel from '../components/ComponentsPanel.vue'
import PhonePanel from '../components/PhonePanel.vue'
import type { LayoutMode } from '../utils/balance'
import AiPanel from '../components/AiPanel.vue'
import SecurityPanel from '../components/SecurityPanel.vue'
import WritingPanel from '../components/WritingPanel.vue'
import ProtocolsPanel from '../components/ProtocolsPanel.vue'
import SettingsDownload from '../components/settings/SettingsDownload.vue'
import SettingsParse from '../components/settings/SettingsParse.vue'
import SettingsAutomation from '../components/settings/SettingsAutomation.vue'
import SettingsDiagnostics from '../components/settings/SettingsDiagnostics.vue'
import SettingsGeneral from '../components/settings/SettingsGeneral.vue'

const app = useAppStore()
const form = ref<Settings>(JSON.parse(JSON.stringify(app.settings!)))
// 表单打开时（或上次同步时）的那份设置：保存时给后端做三方合并；别处改了设置就把改动合进表单
let formBase: Settings = clone(app.settings!)
watch(
  () => app.settingsRev,
  () => {
    if (!app.settings) return
    const merged = merge3(formBase, form.value, app.settings)
    formBase = clone(app.settings)
    if (JSON.stringify(merged) !== JSON.stringify(form.value)) form.value = merged
  },
)
const saving = ref(false)

// 设置页卡片怎么排：自动平衡（默认）/ 按顺序 / 单列。只是这台电脑上的界面偏好，记在本机；在“通用”页切换。
const LAYOUT_KEY = 'cc-settings-layout'
function readLayout(): LayoutMode {
  try {
    const v = localStorage.getItem(LAYOUT_KEY)
    return v === 'order' || v === 'single' ? v : 'balance'
  } catch {
    return 'balance'
  }
}
const CATS = [
  { id: 'download', label: '下载', icon: 'M12 4v11M7 11l5 5 5-5M5 20h14' },
  { id: 'parse', label: '解析', icon: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1' },
  { id: 'p2p', label: 'BT 与 ed2k', icon: 'M6 4v9a6 6 0 0 0 12 0V4h-4v9a2 2 0 0 1-4 0V4zM6 8h4M14 8h4' },
  { id: 'network', label: '网络', icon: 'M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18M12 3a9 9 0 1 0 0 18' },
  { id: 'accounts', label: '账号与 Cookie', icon: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM4 20a8 8 0 0 1 16 0' },
  { id: 'phone', label: '手机与浏览器扩展', icon: 'M8 3h8a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1ZM11 18h2' },
  { id: 'automation', label: '自动化', icon: 'M13 3 5 14h6l-1 7 8-11h-6z' },
  { id: 'ai', label: 'AI', icon: 'M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4' },
  { id: 'writing', label: '写作', icon: 'M4 20l4-1 11-11-3-3L5 16zM14 6l3 3' },
  { id: 'security', label: '安全与隐私', icon: 'M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6l-7-3Z' },
  { id: 'components', label: '组件', icon: 'M12 3 4 7.5v9L12 21l8-4.5v-9L12 3ZM4 7.5 12 12l8-4.5M12 12v9' },
  { id: 'diagnostics', label: '诊断', icon: 'M3 12h4l2-6 4 12 2-6h6' },
  { id: 'general', label: '通用', icon: 'M5 7h14M5 12h14M5 17h14M9 5v4M15 10v4M8 15v4' },
] as const
const catLabel = computed(() => CATS.find((c) => c.id === app.settingsTab)?.label ?? '')
function openPalette() {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true }))
}
const layoutMode = ref<LayoutMode>(readLayout())
watch(layoutMode, (v) => {
  try {
    localStorage.setItem(LAYOUT_KEY, v)
  } catch {
    /* 记不住也不影响 */
  }
})

// 设置一律即时保存：任一项修改后 0.4 秒自动保存，顶部的小标签显示“保存中 / 已自动保存”
let timer: number | undefined
watch(
  form,
  () => {
    window.clearTimeout(timer)
    timer = window.setTimeout(save, 400)
  },
  { deep: true },
)

async function save() {
  saving.value = true
  try {
    await app.save(form.value, formBase)
    formBase = clone(app.settings!)
  } catch (e) {
    ElMessage.error(errorText(e))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="dpage page">
    <div class="dtop">
      <h1>设置</h1>
      <span class="dspacer" />
      <span class="chip" :class="{ cyan: !saving }" role="status" aria-live="polite"><i v-if="!saving" class="ddot" />{{ saving ? '保存中…' : '已自动保存' }}</span>
    </div>

    <div class="dsplit2">
      <aside class="cats dsticky" aria-label="设置分类">
        <button type="button" class="catsearch" aria-label="搜索设置" @click="openPalette">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4-4" /></svg>
          <span>搜索设置</span><span class="kbd">Ctrl K</span>
        </button>
        <button v-for="c in CATS" :key="c.id" type="button" class="cat" :aria-current="app.settingsTab === c.id ? 'true' : 'false'" @click="app.settingsTab = c.id">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="c.icon" /></svg>
          <span>{{ c.label }}</span>
        </button>
      </aside>
      <section class="setmain" :aria-label="`${catLabel}设置`">
        <div class="dtoolbar sethead">
          <h2>{{ catLabel }}</h2>
        </div>
    <el-tabs v-model="app.settingsTab" class="hidetabs">
      <el-tab-pane label="下载" name="download">
        <SettingsDownload v-model="form" :layout="layoutMode" />
      </el-tab-pane>

      <el-tab-pane label="解析" name="parse">
        <SettingsParse v-model="form" :layout="layoutMode" />
      </el-tab-pane>

      <el-tab-pane label="BT 与 ed2k" name="p2p">
        <ProtocolsPanel v-model="form" />
      </el-tab-pane>

      <el-tab-pane label="网络" name="network">
        <NetworkPanel v-model="form.network" />
      </el-tab-pane>

      <el-tab-pane label="账号与 Cookie" name="accounts">
        <AccountsPanel />
      </el-tab-pane>

      <el-tab-pane label="手机与浏览器扩展" name="phone">
        <PhonePanel />
      </el-tab-pane>

      <el-tab-pane label="自动化" name="automation">
        <SettingsAutomation v-model="form" :layout="layoutMode" />
      </el-tab-pane>

      <el-tab-pane label="AI" name="ai">
        <AiPanel v-model="form.ai" />
      </el-tab-pane>

      <el-tab-pane label="写作" name="writing">
        <WritingPanel v-model="form.writing" />
      </el-tab-pane>

      <el-tab-pane label="安全与隐私" name="security">
        <SecurityPanel v-model="form.security" />
      </el-tab-pane>

      <el-tab-pane label="组件" name="components">
        <ComponentsPanel v-model="form.componentMirrors" v-model:edition="form.ffmpegEdition" />
      </el-tab-pane>

      <el-tab-pane label="诊断" name="diagnostics">
        <SettingsDiagnostics v-model="form" :layout="layoutMode" />
      </el-tab-pane>

      <el-tab-pane label="通用" name="general">
        <SettingsGeneral v-model="form" v-model:layout="layoutMode" />
      </el-tab-pane>
    </el-tabs>
      </section>
    </div>
  </div>
</template>

<style scoped>
.dsplit2 {
  display: grid;
  grid-template-columns: 240px minmax(0, 1fr);
  gap: var(--cc-sp-5);
  align-items: start;
}
.cats {
  display: flex;
  flex-direction: column;
  gap: var(--cc-sp-1);
}
.catsearch {
  all: unset;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: var(--cc-sp-3);
  min-height: var(--cc-h-lg);
  padding: 0 var(--cc-sp-3);
  margin-bottom: var(--cc-sp-3);
  border-radius: var(--cc-r-lg);
  border: 1px solid var(--cc-line-strong);
  background: var(--cc-inset);
  color: var(--cc-mute);
  cursor: pointer;
}
.catsearch > span:first-of-type {
  flex: 1;
}
.catsearch:focus-visible,
.cat:focus-visible {
  outline: var(--cc-focus-w) solid var(--cc-focus);
  outline-offset: var(--cc-focus-off);
}
.cat {
  all: unset;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: var(--cc-sp-3);
  width: 100%;
  padding: var(--cc-sp-3);
  border-radius: var(--cc-r-lg);
  border: 1px solid transparent;
  color: var(--cc-mute);
  font-size: var(--cc-fs-md);
  cursor: pointer;
}
.cat svg {
  flex: none;
}
.cat:hover {
  color: var(--cc-fg);
  background: var(--cc-panel);
}
.cat[aria-current='true'] {
  color: var(--cc-acc-text);
  background: var(--cc-panel);
  border-color: var(--cc-line-strong);
  box-shadow: inset 2px 0 0 var(--cc-acc);
}
.setmain {
  min-width: 0;
}
.sethead {
  margin-bottom: var(--cc-sp-4);
}
.sethead h2 {
  margin: 0;
  font-size: var(--cc-fs-xl);
}
.hidetabs :deep(.el-tabs__header) {
  display: none;
}
@media (max-width: 900px) {
  .dsplit2 {
    grid-template-columns: minmax(0, 1fr);
  }
  .cats {
    position: static;
    flex-direction: row;
    flex-wrap: wrap;
  }
  .catsearch {
    width: 100%;
  }
  .cat {
    width: auto;
  }
}
</style>
