<script setup lang="ts">
import {
  NConfigProvider,
  NLoadingBarProvider,
  NDialogProvider,
  NMessageProvider,
  NNotificationProvider,
  zhCN,
  dateZhCN,
} from 'naive-ui'
import Layout from '@/components/Layout/index.vue'


</script>

<template>
  <NConfigProvider
    :locale="zhCN"
    :date-locale="dateZhCN"
  >
    <NLoadingBarProvider>
      <NDialogProvider>
        <NNotificationProvider>
          <NMessageProvider>
            <Layout>
              <RouterView />
            </Layout>
          </NMessageProvider>
        </NNotificationProvider>
      </NDialogProvider>
    </NLoadingBarProvider>
  </NConfigProvider>
</template>