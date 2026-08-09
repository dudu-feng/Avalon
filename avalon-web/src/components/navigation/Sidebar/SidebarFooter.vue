<script setup lang="ts">
import SidebarItem from "./SidebarItem.vue";

const footerItems = [
  {
    label: "记忆",
    icon: "mdi:brain",
    routePath: "/memory",
  },

  {
    label: "知识库",
    icon: "mdi:database",
    routePath: "/knowledge",
  },

  {
    label: "设置",
    icon: "mdi:cog",
    routePath: "/settings",
  },
];
</script>

<template>
  <footer class="av-sidebar-footer">
    <SidebarItem
      v-for="item in footerItems"
      :key="item.routePath"
      :icon="item.icon"
      :label="item.label"
      :route-path="item.routePath"
    />
  </footer>
</template>
