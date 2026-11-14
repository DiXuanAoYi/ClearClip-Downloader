<script setup lang="ts">
// 组件陈列页内容：每个组件一个 section（data-gallery 为组件名），列出所有状态。
import { ref } from 'vue'
import CcButton from './CcButton.vue'
import CcSegmented from './CcSegmented.vue'
import CcField from './CcField.vue'
import CcSelect from './CcSelect.vue'
import CcEmpty from './CcEmpty.vue'
import CcSkeleton from './CcSkeleton.vue'
import CcDialog from './CcDialog.vue'
import CcLink from './CcLink.vue'
import CcToast from './CcToast.vue'
import { notify } from '../../utils/notify'

const seg = ref('balance')
const seg2 = ref('list')
const seg3 = ref('both')
const sel = ref<string | undefined>('general')
const selSmall = ref<string | undefined>('general')
const selEmpty = ref<string | undefined>(undefined)
const selMany = ref<string | undefined>('m3')
const text = ref('http://localhost:11434')
const sw = ref(true)
const loading = ref(false)
const clicks = ref(0)
const retried = ref(0)
const dlgSm = ref(false)
const dlgMd = ref(false)
const dlgLg = ref(false)
const scenes = [
  { value: 'general', label: '通用' },
  { value: 'xhs', label: '小红书' },
  { value: 'weibo', label: '微博' },
  { value: 'weekly', label: '周报', disabled: true },
]
const many = Array.from({ length: 12 }, (_, i) => ({ value: `m${i}`, label: `模型 ${i + 1}` }))
const longDesc = '指定使用哪个专家。“通用”适合大多数场景；翻译字幕、网页时会按网站选择对应的专家，也可以在这里手动指定。译文格式始终由程序保证，专家的说明只会补充用词和语气，不会改变输出的结构，也不会影响计费方式或者速度，所以放心选择。'
function startLoading() {
  loading.value = true
  setTimeout(() => (loading.value = false), 1500)
}
</script>

<template>
  <main class="gal">
    <h1>清影组件库 <small class="mono">components/ui</small></h1>

    <section data-gallery="CcButton" class="card sec">
      <h2>CcButton</h2>
      <div class="row" data-row="kinds">
        <CcButton kind="primary">添加订阅</CcButton>
        <CcButton>次按钮</CcButton>
        <CcButton kind="text">文字按钮</CcButton>
        <CcButton kind="danger">取消下载</CcButton>
        <CcButton kind="primary" disabled disabled-reason="先选择一个视频">禁用主按钮</CcButton>
        <CcButton disabled>禁用</CcButton>
      </div>
      <div class="row" data-row="sizes">
        <CcButton kind="primary" size="s">小</CcButton>
        <CcButton kind="primary" size="m">中</CcButton>
        <CcButton kind="primary" size="l">大</CcButton>
        <CcButton size="s">小</CcButton>
        <CcButton size="l">大</CcButton>
      </div>
      <div class="row" data-row="icons">
        <CcButton kind="primary"><template #icon><svg width="14" height="14" viewBox="0 0 14 14"><path d="M7 2v10M2 7h10" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" /></svg></template>带图标</CcButton>
        <CcButton icon-only aria-label="更多操作"><template #icon><svg width="16" height="16" viewBox="0 0 16 16"><circle cx="3" cy="8" r="1.4" fill="currentColor" /><circle cx="8" cy="8" r="1.4" fill="currentColor" /><circle cx="13" cy="8" r="1.4" fill="currentColor" /></svg></template></CcButton>
        <CcButton icon-only kind="text" size="s" aria-label="关闭"><template #icon><svg width="12" height="12" viewBox="0 0 12 12"><path d="m2 2 8 8m0-8-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg></template></CcButton>
        <CcButton icon-only size="l" disabled aria-label="禁用的图标按钮"><template #icon><svg width="18" height="18" viewBox="0 0 18 18"><circle cx="9" cy="9" r="6" fill="none" stroke="currentColor" stroke-width="1.6" /></svg></template></CcButton>
      </div>
      <div class="row" data-row="loading">
        <CcButton data-testid="btn-loading" kind="primary" :loading="loading" @click="startLoading(); clicks++">保存设置</CcButton>
        <CcButton data-testid="btn-loading-static" :loading="true" @click="clicks += 100">检测模型</CcButton>
        <span class="sm">点击次数 <b data-testid="click-count">{{ clicks }}</b></span>
      </div>
    </section>

    <section data-gallery="CcSegmented" class="card sec">
      <h2>CcSegmented</h2>
      <div class="row">
        <CcSegmented v-model="seg" aria-label="卡片排布" :options="[{ value: 'balance', label: '自动平衡' }, { value: 'order', label: '按顺序' }, { value: 'one', label: '单列' }]" />
        <CcSegmented v-model="seg2" size="s" aria-label="视图" :options="[{ value: 'list', label: '列表' }, { value: 'grid', label: '网格' }]" />
        <CcSegmented v-model="seg3" aria-label="显示" :options="[{ value: 'orig', label: '原页', disabled: true, hint: '这个页面没有原文' }, { value: 'tr', label: '译文' }, { value: 'both', label: '原页+译文' }]" />
        <span class="sm">当前：<b data-testid="seg-val">{{ seg }}</b></span>
      </div>
    </section>

    <section data-gallery="CcLink" class="card sec">
      <h2>CcLink</h2>
      <p class="sm">可以自建，或从 <CcLink href="https://example.test/glossary" external>公共术语库</CcLink> 安装。<CcLink>了解更多</CcLink> · <CcLink disabled>不可用</CcLink></p>
    </section>

    <section data-gallery="CcField" class="card sec">
      <h2>CcField</h2>
      <CcField label="默认专家" desc="“通用”适合大多数场景。">
        <template #default="{ id }"><CcSelect :id="id" v-model="sel" :options="scenes" aria-label="默认专家" /></template>
      </CcField>
      <CcField label="自动选用专家" :desc="longDesc" data-testid="field-long">
        <template #default="{ id }"><el-switch :id="id" v-model="sw" aria-label="自动选用" /></template>
      </CcField>
      <CcField label="模型接口地址" desc="以 https:// 开头，不要带结尾的 /v1。" error="地址不能访问，检查端口是否正确（连接被拒绝）">
        <template #default="{ id, descId, errId, invalid }"><input :id="id" v-model="text" class="gal-input" :aria-describedby="`${descId} ${errId}`" :aria-invalid="invalid" /></template>
      </CcField>
      <CcField label="术语库" desc="让专有名词保持同一个译法。" layout="col">
        <CcButton size="s">新建术语库</CcButton><CcButton size="s">新建文件夹</CcButton><CcButton size="s">导入 CSV…</CcButton>
      </CcField>
    </section>

    <section data-gallery="CcSelect" class="card sec">
      <h2>CcSelect</h2>
      <div class="row">
        <CcSelect v-model="sel" :options="scenes" aria-label="场景" />
        <CcSelect v-model="selSmall" size="s" width="s" :options="scenes" aria-label="场景（小）" />
        <CcSelect v-model="selEmpty" :options="scenes" placeholder="请选择场景" aria-label="未选择" />
        <CcSelect v-model="sel" :options="scenes" invalid aria-label="校验失败" />
        <CcSelect v-model="sel" :options="scenes" disabled aria-label="已禁用" />
        <CcSelect v-model="selMany" :options="many" aria-label="多项（带搜索）" />
      </div>
    </section>

    <section data-gallery="CcEmpty" class="card sec">
      <h2>CcEmpty</h2>
      <CcEmpty title="还没有订阅" desc="粘贴频道、UP 主或播放列表链接，有新内容时自动下载。">
        <CcButton kind="primary">添加订阅</CcButton>
      </CcEmpty>
      <CcEmpty compact title="模型池是空的" desc="添加一个有免费额度的服务就能开始。">
        <CcButton kind="primary" size="s">添加服务</CcButton>
      </CcEmpty>
    </section>

    <section data-gallery="CcSkeleton" class="card sec">
      <h2>CcSkeleton</h2>
      <div class="row nowrap">
        <CcSkeleton width="96px" :height="54" radius="6px" />
        <div class="grow"><CcSkeleton width="70%" :height="14" /><div style="height: 8px" /><CcSkeleton :lines="2" :height="10" /></div>
        <CcSkeleton circle :height="32" />
      </div>
    </section>

    <section data-gallery="CcDialog" class="card sec">
      <h2>CcDialog</h2>
      <div class="row">
        <CcButton data-testid="open-sm" @click="dlgSm = true">确认（sm 400）</CcButton>
        <CcButton data-testid="open-md" @click="dlgMd = true">表单（md 560）</CcButton>
        <CcButton data-testid="open-lg" @click="dlgLg = true">列表（lg 720）</CcButton>
      </div>
      <CcDialog v-model="dlgSm" title="取消这个下载？" size="sm" danger initial-focus="[data-keep]">
        <span>“【4K 延时】上海 · 夜未央”已下载 28.6 MB，取消后会保留已下载的部分，可以稍后继续。</span>
        <template #footer><CcButton data-keep @click="dlgSm = false">先不取消</CcButton><CcButton kind="danger" @click="dlgSm = false">取消下载</CcButton></template>
      </CcDialog>
      <CcDialog v-model="dlgMd" title="添加服务" size="md">
        <CcField label="服务商"><CcSelect v-model="sel" :options="scenes" width="l" aria-label="服务商" /></CcField>
        <CcField label="密钥" desc="保存在本机，不进设置文件和备份。"><input class="gal-input" type="password" aria-label="密钥" /></CcField>
        <template #footer><CcButton @click="dlgMd = false">取消</CcButton><CcButton kind="primary" @click="dlgMd = false">检测并保存</CcButton></template>
      </CcDialog>
      <CcDialog v-model="dlgLg" title="选择模型" size="lg">
        <p v-for="i in 30" :key="i" class="sm" style="margin: 0">模型 {{ i }}：一行说明文字，用来撑出滚动。</p>
        <template #footer><CcButton kind="primary" @click="dlgLg = false">完成</CcButton></template>
      </CcDialog>
    </section>

    <section data-gallery="CcToast" class="card sec">
      <h2>CcToast</h2>
      <div class="col">
        <CcToast kind="error" title="字幕下载失败" desc="连接被重置（HTTP 403）。可能需要登录或更换节点。" detail="error sending request for url (https://example.test/sub.vtt): connection reset" retryable @retry="retried++" />
        <CcToast kind="success" title="已加入下载队列" />
        <CcToast kind="warning" title="磁盘剩余空间不足 2 GB" desc="继续下载可能失败。" />
        <CcToast kind="info" title="已复制链接" />
      </div>
      <div class="row">
        <CcButton data-testid="fire-toast" @click="notify({ kind: 'error', title: '无法保存设置', desc: '写入 settings.json 被拒绝：磁盘只读。', detail: 'EROFS: read-only file system', onRetry: () => retried++ })">弹出错误提示</CcButton>
        <span class="sm">重试次数 <b data-testid="retry-count">{{ retried }}</b></span>
      </div>
    </section>
  </main>
</template>

<style>
.gal { max-width: 960px; margin: 0 auto; padding: var(--cc-sp-6) var(--cc-sp-4) var(--cc-sp-12); display: flex; flex-direction: column; gap: var(--cc-sp-4); font-size: var(--cc-fs-md); }
.gal h1 { margin: 0; font-size: var(--cc-fs-xl); }
.gal h1 small { font-size: var(--cc-fs-xs); color: var(--cc-faint); margin-left: var(--cc-sp-2); }
.gal .sec { padding: var(--cc-sp-5); display: flex; flex-direction: column; gap: var(--cc-sp-3); min-width: 0; }
.gal .sec h2 { margin: 0; font-size: var(--cc-fs-md); font-weight: 700; }
.gal .row { display: flex; flex-wrap: wrap; gap: var(--cc-sp-3); align-items: center; }
.gal .row.nowrap { flex-wrap: nowrap; }
.gal .col { display: flex; flex-direction: column; gap: var(--cc-sp-3); align-items: flex-start; }
.gal .grow { flex: 1; min-width: 0; }
.gal .sm { font-size: var(--cc-fs-sm); color: var(--cc-mute); }
.gal-input { height: var(--cc-h-md); padding: 0 var(--cc-sp-3); min-width: 0; width: 240px; max-width: 100%; border-radius: var(--cc-r-md); background: var(--cc-fill); border: 1px solid var(--cc-line-strong); color: var(--cc-fg); font: inherit; font-size: var(--cc-fs-md); box-sizing: border-box; }
.gal-input[aria-invalid='true'] { border-color: var(--cc-err); }
</style>
