import { computed, ref } from 'vue'

export type CursorValue = string | undefined

export function useCursorPagination(initialPageSize: number) {
  const currentPage = ref(1)
  const pageSize = ref(initialPageSize)
  const cursorStack = ref<CursorValue[]>([undefined])
  const currentCursor = computed(() => cursorStack.value[currentPage.value - 1])

  function reset(page = 1) {
    currentPage.value = page
    cursorStack.value = [undefined]
  }

  function goToNext(nextCursor?: string | null) {
    if (!nextCursor) return false
    cursorStack.value[currentPage.value] = nextCursor
    currentPage.value += 1
    return true
  }

  function goToPrevious() {
    if (currentPage.value <= 1) return false
    currentPage.value -= 1
    return true
  }

  return {
    currentPage,
    pageSize,
    currentCursor,
    reset,
    goToNext,
    goToPrevious
  }
}
