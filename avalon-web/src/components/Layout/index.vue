<template>
  <div class="layout-container flex min-h-screen bg-gray-50">
    <!-- 侧边栏 -->
    <Sidebar />
    <!-- 主内容区域 -->
    <main class="flex-1 p-6 overflow-auto">
      <slot />
    </main>
  </div>
</template>

<script setup lang="ts">
import Sidebar from './Sidebar/index.vue'
</script>