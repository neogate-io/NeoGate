import { computed, ref, type Ref } from 'vue'

export function useReactiveSet<T>() {
  const items = ref(new Set<T>()) as Ref<Set<T>>
  const size = computed(() => items.value.size)

  function has(value: T) {
    return items.value.has(value)
  }

  function add(value: T) {
    if (items.value.has(value)) return
    items.value = new Set(items.value).add(value)
  }

  function remove(value: T) {
    if (!items.value.has(value)) return
    const next = new Set(items.value)
    next.delete(value)
    items.value = next
  }

  function set(values: Iterable<T>) {
    items.value = new Set(values)
  }

  function toggle(value: T, enabled = !has(value)) {
    if (enabled) {
      add(value)
    } else {
      remove(value)
    }
  }

  function retain(predicate: (value: T) => boolean) {
    set([...items.value].filter(predicate))
  }

  async function withItem<R>(value: T, task: () => Promise<R>) {
    add(value)
    try {
      return await task()
    } finally {
      remove(value)
    }
  }

  return {
    items,
    size,
    has,
    add,
    remove,
    set,
    toggle,
    retain,
    withItem
  }
}
