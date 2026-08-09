<template>
  <div class="sidebar-header flex items-center justify-between px-4 py-5 border-b border-gray-100">
    <div class="logo flex items-center gap-2" v-if="!collapsed">
      <span class="i-mdi-book-open-outline text-2xl text-blue-500"></span>
      <span class="text-xl font-semibold bg-gradient-to-r from-blue-500 to-orange-400 bg-clip-text text-transparent">
        Avalon
      </span>
    </div>
    <div v-else class="w-full flex justify-center">
      <span class="i-mdi-book-open-outline text-2xl text-blue-500"></span>
    </div>

    <!-- 折叠切换按钮 -->
    <span
      class="i-mdi-chevron-left cursor-pointer text-xl text-gray-600 hover:text-blue-500"
      @click="toggleCollapse"
    ></span>
  </div>
</template>

<script setup lang="ts">
import { inject } from 'vue'
import type { Ref } from 'vue'

const collapsed = inject<Ref<boolean>>('sidebarCollapsed')!
const emit = defineEmits(['toggle'])

const toggleCollapse = () => {
  emit('toggle')
}
</script>