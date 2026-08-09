<template>
  <aside
    class="sidebar h-screen sticky top-0 bg-white border-r border-gray-200 flex flex-col transition-all duration-300 shadow-sm"
    :class="collapsed ? 'w-16' : 'w-64'"
  >
    <!-- 头部 Logo + 折叠按钮 -->
    <SidebarHeader @toggle="handleToggle" />

    <!-- 菜单主体区域 -->
    <div class="menu-body flex-1 overflow-y-auto py-4 px-2 space-y-1">
      <MenuItem icon-class="i-mdi-home" label="首页" route-path="/" />

      <!-- Recents 可折叠分组 -->
      <div class="mt-4 px-3 flex items-center justify-between text-gray-400 text-sm">
        <span v-if="!collapsed">Recents</span>
        <span class="i-mdi-chevron-right text-sm"></span>
      </div>
    </div>

    <!-- 底部固定菜单 -->
    <SidebarFooter />
  </aside>
</template>

<script setup lang="ts">
import { ref, provide } from 'vue'
import SidebarHeader from './Header/index.vue'
import SidebarFooter from './Footer/index.vue'
import MenuItem from './MenuItem/index.vue'

// 侧边栏折叠状态
const collapsed = ref(false)

// 向下注入状态给所有子组件
provide('sidebarCollapsed', collapsed)

const handleToggle = () => {
  collapsed.value = !collapsed.value
}
</script>