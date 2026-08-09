import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

export interface SessionMeta {
  id: string
  title: string
  createdAt: number
  updatedAt: number
}

export const useSessionStore = defineStore('session', () => {
  const sessions = ref<SessionMeta[]>([])
  const currentId = ref<string | null>(null)

  const current = computed(
    () => sessions.value.find(s => s.id === currentId.value) ?? null,
  )

  function setCurrent(id: string | null) {
    currentId.value = id
  }

  function upsert(meta: SessionMeta) {
    const idx = sessions.value.findIndex(s => s.id === meta.id)
    if (idx >= 0) sessions.value[idx] = meta
    else sessions.value.unshift(meta)
  }

  function remove(id: string) {
    sessions.value = sessions.value.filter(s => s.id !== id)
    if (currentId.value === id) currentId.value = null
  }

  return { sessions, currentId, current, setCurrent, upsert, remove }
})