<script setup lang="ts">
import { AvIcon, AvLogo } from "@/components/ui";

interface Props {
  collapsed: boolean;
}

defineProps<Props>();

const emit = defineEmits<{
  toggle: [];
}>();

const handleToggle = () => {
  emit("toggle");
};
</script>

<template>
  <header class="av-sidebar-header">
    <!-- Brand -->

    <div class="av-sidebar-brand">
      <AvLogo class="av-sidebar-brand-logo" size="sm" />

      <span class="av-sidebar-brand-name"> Avalon </span>
    </div>

    <!-- Collapse -->

    <button
      class="av-sidebar-collapse"
      type="button"
      aria-label="折叠侧边栏"
      @click="handleToggle"
    >
      <AvIcon
        name="mdi:chevron-left"
        :size="18"
        :class="{
          'rotate-180': collapsed,
        }"
      />
    </button>
  </header>
</template>
