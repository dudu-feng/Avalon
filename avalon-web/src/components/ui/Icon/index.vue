<script setup lang="ts">

import { Icon } from '@iconify/vue'


interface Props {

  /**
   * 图标名称
   * 示例:
   * mdi:chat
   * mdi:robot
   * mdi:cog
   */
  name: string


  /**
   * 图标尺寸
   */
  size?: number | string


  /**
   * 图标颜色
   */
  color?: string

}


const props = withDefaults(
  defineProps<Props>(),
  {
    size: 20,
  }
)

</script>


<template>

  <Icon

    :icon="props.name"

    :width="props.size"

    :height="props.size"

    :color="props.color"

  />

</template>