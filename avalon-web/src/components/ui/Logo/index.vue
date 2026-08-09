<script setup lang="ts">
import logo from "@/assets/logo/Avalon-transparent.png";

interface Props {
  size?: "sm" | "md" | "lg";
}

const props = withDefaults(defineProps<Props>(), {
  size: "md",
});

const sizes = {
  sm: "32px",

  md: "40px",

  lg: "64px",
};
</script>

<template>
  <img
    :src="logo"
    :style="{
      width: sizes[props.size],

      height: sizes[props.size],
    }"
    class="object-contain"
  />
</template>
