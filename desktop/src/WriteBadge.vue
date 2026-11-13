<script setup lang="ts">
import { api } from './api'

/** 写作浮标（桌面端）：很小的圆形按钮；窗口本身不抢焦点，点一下弹出写作窗。 */
function click() {
  void api.writeBadgeClick()
}
</script>

<template>
  <button class="b" title="清影写作" aria-label="清影写作" @mousedown.prevent @click="click">写</button>
</template>

<style>
html, body { margin: 0; background: transparent !important; overflow: hidden; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.b { width: 100%; height: 100%; border: 0; border-radius: 50%; background: #b45f00; color: #fff; font: 700 13px/1 -apple-system, 'PingFang SC', 'Microsoft YaHei', 'Noto Sans SC', sans-serif; cursor: pointer; box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.85), 0 2px 6px rgba(180, 86, 26, 0.55); padding: 0; transition: background 0.12s; }
.b:hover { background: #b45f00; }
</style>
