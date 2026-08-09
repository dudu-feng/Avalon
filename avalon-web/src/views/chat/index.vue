<script setup lang="ts">
import { useRoute } from 'vue-router'
import { useSessionStore } from '@/stores/session'

const route = useRoute()
const sessionStore = useSessionStore()
const sessionId = route.params.sessionId as string | undefined
</script>

<template>
  <div class="chat-view">
    <h1>Chat</h1>
    <p>当前会话：{{ sessionId || '新对话' }}</p>
    <p>会话总数：{{ sessionStore.sessions.length }}</p>
  </div>
</template>