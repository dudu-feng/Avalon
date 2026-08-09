<template>
  <div class="sidebar-header flex items-center px-4 py-4 border-b border-gray-100 min-h-[64px]">
    <!-- Logo 区域：同一个 DOM，折叠时只隐藏文字避免跳变 -->
    <div class="logo flex items-center gap-3 flex-1 overflow-hidden">
      <!-- 图标：始终显示，折叠态不位移 -->
      <span class="i-mdi-book-open-outline text-2xl text-blue-500 flex-shrink-0"></span>
      <!-- 文字：折叠时用 opacity + width 过渡隐藏 -->
      <span
        class="text-lg font-semibold bg-gradient-to-r from-blue-500 to-orange-400 bg-clip-text text-transparent whitespace-nowrap transition-all ease-in-out duration-300"
        :class="collapsed ? 'opacity-0 w-0 -ml-3' : 'opacity-100 w-auto'"
      >
        Avalon
      </span>
    </div>

    <!-- 折叠切换按钮：添加旋转动画 -->
    <span
      class="i-mdi-chevron-left cursor-pointer text-xl text-gray-500 hover:text-blue-500 transition-all duration-300 ease-in-out p-1 rounded hover:bg-gray-50"
      :class="collapsed ? 'rotate-180' : 'rotate-0'"
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