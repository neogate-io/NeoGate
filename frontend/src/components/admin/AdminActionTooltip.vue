<script setup lang="ts">
withDefaults(
  defineProps<{
    content: string
    placement?: 'top' | 'top-start' | 'top-end' | 'bottom' | 'bottom-start' | 'bottom-end'
    showAfter?: number
  }>(),
  {
    placement: 'top',
    showAfter: 600
  }
)
</script>

<template>
  <el-tooltip :content="content" :placement="placement" :show-after="showAfter">
    <slot />
  </el-tooltip>
</template>
