<script setup lang="ts">
import type { Settings } from '../../types'
import type { LayoutMode } from '../../utils/balance'
import BalancedGrid from '../BalancedGrid.vue'
import NotifyPanel from '../NotifyPanel.vue'
import UploadPanel from '../UploadPanel.vue'
import RulesPanel from '../RulesPanel.vue'

const form = defineModel<Settings>({ required: true })
defineProps<{ layout: LayoutMode }>()
</script>

<template>
  <BalancedGrid class="groups" :mode="layout">
          <NotifyPanel v-model="form.notify" />
          <UploadPanel v-model="form.upload" />
          <RulesPanel v-model="form.rules" class="wide" />
  </BalancedGrid>
</template>

<style scoped src="./settings-shared.css"></style>
