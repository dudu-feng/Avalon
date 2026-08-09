<template>
  <div
    class="menu-item flex items-center gap-3 px-4 py-3 rounded-lg cursor-pointer transition-all duration-200"
    :class="[
      isActive ? 'bg-gray-100 text-blue-600' : 'hover:bg-gray-50 text-gray-700',
      sidebarCollapsed ? 'justify-center px-2' : ''
    ]"
    @click="$router.push(routePath)"
  >
    <!-- Iconify 图标，UnoCSS 直接用 class 渲染 -->
    <span :class="iconClass" class="w-5 h-5 flex-shrink-0"></span>
    <!-- 折叠时隐藏文字 -->
    <span v-if="!sidebarCollapsed" class="text-base whitespace-nowrap">{{ label }}</span>
  </div>
</template>

<script setup lang="ts">
import { inject, computed } from 'vue'
import type { Ref } from 'vue'
import { useRoute } from 'vue-router'

const props = defineProps<{
  iconClass: string // 图标类，如 i-mdi-home
  label: string
  routePath: string
}>()

// 注入父组件的折叠状态
const sidebarCollapsed = inject<Ref<boolean>>('sidebarCollapsed')!
// 判断路由是否激活（可根据vue-router优化）
const isActive = computed(() => useRoute().path === props.routePath)
</script>