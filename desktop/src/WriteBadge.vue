<script setup lang="ts">
import { api } from './api'

/** 写作浮标（桌面端）：很小的圆形按钮；窗口本身不抢焦点，点一下弹出写作窗。 */
function click() {
  void api.writeBadgeClick()
}
</script>

<template>
  <button class="b" title="清影写作" @mousedown.prevent @click="click">写</button>
</template>

<style>
html, body { margin: 0; background: transparent !important; overflow: hidden; }
#app { width: 100vw; height: 100vh; }
</style>

<style scoped>
.b { width: 100%; height: 100%; border: 0; border-radius: 50%; background: #0f766e; color: #fff; font: 600 13px system-ui, 'Noto Sans SC', sans-serif; cursor: pointer; box-shadow: 0 1px 4px rgba(0, 0, 0, 0.35); padding: 0; }
.b:hover { background: #115e59; }
</style>
