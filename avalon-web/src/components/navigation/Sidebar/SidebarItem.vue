<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";

import { AvIcon } from "@/components/ui";

interface Props {
  icon: string;
  label: string;
  routePath: string;
}

const props = defineProps<Props>();

const route = useRoute();
const router = useRouter();

const isActive = computed(() => {
  return route.path === props.routePath;
});

const handleClick = () => {
  router.push(props.routePath);
};
</script>

<template>
  <button
    class="av-sidebar-item"
    :class="{
      'is-active': isActive,
    }"
    type="button"
    @click="handleClick"
  >
    <span class="av-sidebar-item-icon">
      <AvIcon :name="props.icon" :size="20" />
    </span>

    <span class="av-sidebar-item-label">
      {{ props.label }}
    </span>
  </button>
</template>
