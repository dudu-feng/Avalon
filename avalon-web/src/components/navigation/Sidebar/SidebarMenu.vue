<script setup lang="ts">
import SidebarItem from "./SidebarItem.vue";

export interface SidebarMenuItem {
  label: string;
  icon: string;
  routePath: string;
}

interface Props {
  items: SidebarMenuItem[];
}

defineProps<Props>();
</script>

<template>
  <nav class="av-sidebar-menu">
    <SidebarItem
      v-for="item in items"
      :key="item.routePath"
      :icon="item.icon"
      :label="item.label"
      :route-path="item.routePath"
    />

    <div class="av-sidebar-section">
      <span> Recents </span>

      <span>
        <span class="i-carbon-chevron-right"></span>
      </span>
    </div>

    <slot />
  </nav>
</template>
