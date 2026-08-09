<template>
  <div class="sidebar-footer mt-auto border-t border-gray-100 px-2 py-4 space-y-1">
    <MenuItem
      icon-class="i-mdi-brain"
      label="记忆"
      route-path="/memory"
    />
    <MenuItem
      icon-class="i-mdi-book-open-page-variant"
      label="知识库"
      route-path="/knowledge"
    />
    <MenuItem
      icon-class="i-mdi-cog-outline"
      label="设置"
      route-path="/settings"
    />
  </div>
</template>

<script setup lang="ts">
import MenuItem from '../MenuItem/index.vue'
</script>