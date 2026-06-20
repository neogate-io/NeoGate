import type { Ref } from 'vue'
import type { CursorValue } from './useCursorPagination'

type CursorPaginationControls = {
  pageSize: Ref<number>
  reset: (page?: number) => void
  goToNext: (nextCursor?: string | null) => boolean
  goToPrevious: () => boolean
}

type CursorPageState = {
  has_more?: boolean
  next_cursor?: CursorValue | null
}

export function useCursorPageActions(
  pagination: CursorPaginationControls,
  getPage: () => CursorPageState,
  reload: () => Promise<unknown>
) {
  async function resetAndReload(page = 1) {
    pagination.reset(page)
    await reload()
  }

  async function nextPage() {
    const page = getPage()
    if (!page.has_more || !page.next_cursor) return
    if (!pagination.goToNext(page.next_cursor)) return
    await reload()
  }

  async function previousPage() {
    if (!pagination.goToPrevious()) return
    await reload()
  }

  async function handlePageSizeChange(size: number) {
    pagination.pageSize.value = size
    await resetAndReload()
  }

  return {
    resetAndReload,
    nextPage,
    previousPage,
    handlePageSizeChange
  }
}
