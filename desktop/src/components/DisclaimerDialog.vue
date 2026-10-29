<script setup lang="ts">
import { computed, ref } from 'vue'
import { useAppStore } from '../stores/app'

const app = useAppStore()
const saving = ref(false)
const visible = computed(() => !!app.settings && !app.settings.disclaimerAccepted)

async function accept() {
  saving.value = true
  try {
    await app.patch({ disclaimerAccepted: true })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <el-dialog
    :model-value="visible"
    title="使用前请阅读"
    width="460px"
    :show-close="false"
    :close-on-click-modal="false"
    :close-on-press-escape="false"
    align-center
  >
    <div class="body">
      <p>清影用于把你在抖音、B站、YouTube 等视频和图片网站上看到的作品保存到本地，方便个人收藏和学习。</p>
      <ul>
        <li>只下载你有权保存的内容，例如自己发布的作品，或作者允许下载的内容。</li>
        <li>不要把下载的内容二次发布、搬运或用于商业用途。</li>
        <li>解析依赖平台公开页面，平台调整后可能暂时无法使用。</li>
        <li>所有数据（历史记录、登录 Cookie、设置）只保存在这台电脑上。</li>
      </ul>
      <p class="mute">本项目仅供学习研究。如涉及侵权，请联系仓库作者删除。</p>
    </div>
    <template #footer>
      <el-button type="primary" :loading="saving" @click="accept">我已了解，开始使用</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.body {
  line-height: 1.75;
}
.body ul {
  padding-left: 18px;
  margin: 8px 0;
}
</style>
