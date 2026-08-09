<script setup lang="ts">
import Sidebar from "@/components/navigation/Sidebar/index.vue";
</script>

<template>
  <div class="av-layout">
    <Sidebar />

    <main class="av-layout-main">
      <slot />
    </main>
  </div>
</template>
