import { ref } from 'vue'
import { api, describe } from '@/api/client'
import type { ListParams } from '@/api/types'

/** One server page of a list endpoint (go-tangra list contract, specs 032). */
export interface Page<T> {
  items: T[]
  total: number
  page: number
  page_size: number
  sort: string
  order: 'asc' | 'desc'
}

/** A filter value: a string, or a list sent as a repeated parameter (`tag=a&tag=b`). */
export type FilterValue = string | string[] | undefined
export type ListFilter = Record<string, FilterValue>

/** `path?k=v&k=w` for the list-valued filter entries (the kit client sends scalars only). */
export function withRepeated(path: string, f: ListFilter): { path: string; query: Record<string, string | undefined> } {
  const q = new URLSearchParams()
  const query: Record<string, string | undefined> = {}
  for (const [k, v] of Object.entries(f)) {
    if (Array.isArray(v)) v.filter(Boolean).forEach((x) => q.append(k, x))
    else query[k] = v
  }
  const s = q.toString()
  return { path: s ? path + '?' + s : path, query }
}

/**
 * Server-paged list state: the current page's rows, the total, the last page
 * parameters and filter. A response superseded by a newer request is ignored
 * (rapid paging, sorting or typing).
 */
export function pagedList<T, F extends ListFilter>(path: string, first: ListParams) {
  const items = ref<T[]>([])
  const total = ref(0)
  const params = ref<ListParams>({ ...first })
  const filter = ref<ListFilter>({})
  const loading = ref(false)
  const error = ref('')
  let seq = 0

  /** Loads one page; resolves with it, or null when it failed or was superseded. */
  async function list(f: F = filter.value as F, q: ListParams = params.value): Promise<Page<T> | null> {
    const mine = ++seq
    loading.value = true
    error.value = ''
    filter.value = { ...f }
    params.value = { ...q }
    try {
      const r = withRepeated(path, f)
      const res = await api<Page<T>>('GET', r.path, undefined, { query: { ...r.query, ...q } })
      if (mine !== seq) return null
      items.value = (res.items ?? []) as never
      total.value = res.total ?? 0
      return res
    } catch (e) {
      if (mine === seq) error.value = describe(e)
      return null
    } finally {
      if (mine === seq) loading.value = false
    }
  }

  return { items, total, params, filter, loading, error, list, reload: () => list() }
}
