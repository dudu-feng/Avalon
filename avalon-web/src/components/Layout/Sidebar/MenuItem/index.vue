<template>
  <div
    class="menu-item group relative flex items-center gap-3 px-3 rounded-lg cursor-pointer transition-all duration-200 min-h-[42px]"
    :class="[
      isActive
        ? 'bg-blue-50 text-blue-600'
        : 'hover:bg-gray-100 text-gray-700',
    ]"
    @click="$router.push(routePath)"
  >
    <!-- 左侧激活指示条：激活态显示 -->
    <span
      class="absolute left-0 top-1/2 -translate-y-1/2 w-1 h-6 rounded-r bg-blue-500 transition-opacity duration-200"
      :class="isActive ? 'opacity-100' : 'opacity-0'"
    ></span>

    <!-- 图标：固定 left padding，折叠时不位移 -->
    <span :class="iconClass" class="w-5 h-5 flex-shrink-0 ml-1"></span>

    <!-- 文字：折叠时用 opacity + width 过渡，不用 v-if 避免突变 -->
    <span
      class="text-sm whitespace-nowrap transition-all ease-in-out duration-300 overflow-hidden"
      :class="sidebarCollapsed ? 'opacity-0 w-0' : 'opacity-100 w-auto'"
    >
      {{ label }}
    </span>
  </div>
</template>

<script setup lang="ts">
import { inject, computed } from 'vue'
import type { Ref } from 'vue'
import { useRoute } from 'vue-router'

const props = defineProps<{
  iconClass: string
  label: string
  routePath: string
}>()

// 先在顶层获取路由实例，不要放进 computed 里面重复调用
const route = useRoute()

// 注入折叠状态
const sidebarCollapsed = inject<Ref<boolean>>('sidebarCollapsed')!

// 用可选链 ?. 安全访问 path，route 不存在直接返回 false
const isActive = computed(() => route?.path === props.routePath)
</script>