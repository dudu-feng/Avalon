<script setup lang="ts">
import { ref } from "vue";

import SidebarHeader from "./SidebarHeader.vue";
import SidebarMenu from "./SidebarMenu.vue";
import SidebarFooter from "./SidebarFooter.vue";

const collapsed = ref(false);

const menuItems = [
  {
    label: "Chat",
    icon: "mdi:chat",
    routePath: "/chat",
  }
];

const toggleCollapse = () => {
  collapsed.value = !collapsed.value;
};
</script>

<template>
  <aside
    class="av-sidebar"
    :class="{
      'is-collapsed': collapsed,
    }"
  >
    <!-- Header -->

    <SidebarHeader :collapsed="collapsed" @toggle="toggleCollapse" />

    <!-- Menu -->

    <SidebarMenu :items="menuItems" />

    <!-- Footer -->

    <SidebarFooter />
  </aside>
</template>
