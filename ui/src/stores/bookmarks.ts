import { defineStore } from 'pinia'
import type { ListQueryOptions } from '@go-tangra/ui'
import { api } from '@/api/client'
import type { Bookmark, BookmarkPatch, Effective, NewBookmark } from '@/api/types'
import { pagedList } from '@/stores/paged'

export const PAGE_SIZE = 25

/** Sortable fields of GET /bookmarks (default created_at desc). */
export const BOOKMARK_SORTS = ['created_at', 'updated_at', 'title', 'url'] as const
export const BOOKMARK_LIST: ListQueryOptions = { sortable: [...BOOKMARK_SORTS], defaultSort: { key: 'created_at', dir: 'desc' }, defaultSize: PAGE_SIZE }

export interface BookmarkFilter extends Record<string, string | string[] | undefined> {
  q?: string | undefined
  tag?: string[] | undefined
}

export const useBookmarks = defineStore('bookmark-bookmarks', () => {
  const page = pagedList<Bookmark, BookmarkFilter>('bookmarks', { page: 1, page_size: PAGE_SIZE, sort: 'created_at', order: 'desc' })

  const get = (id: string) => api<Bookmark>('GET', 'bookmarks/' + encodeURIComponent(id))

  // A new bookmark takes its place in the server's order: the caller reloads the page.
  const create = (input: NewBookmark) => api<Bookmark>('POST', 'bookmarks', input)

  async function update(id: string, patch: BookmarkPatch): Promise<Bookmark> {
    const b = await api<Bookmark>('PUT', 'bookmarks/' + encodeURIComponent(id), patch)
    page.items.value = page.items.value.map((x) => (x.id === id ? b : x))
    return b
  }

  async function remove(id: string): Promise<void> {
    await api('POST', 'bookmarks/' + encodeURIComponent(id) + '/remove')
    page.items.value = page.items.value.filter((x) => x.id !== id)
  }

  /** The caller's own access to one bookmark (or, without an id, the collection). */
  const effective = (id = '') => api<Effective>('GET', 'permissions/effective', undefined, { query: { resource_id: id } })

  /** Bookmarks matching `q` (title or address), for pickers. */
  async function search(q: string, size = 20): Promise<Bookmark[]> {
    const res = await api<{ items: Bookmark[] }>('GET', 'bookmarks', undefined, { query: { q: q.trim(), page_size: size, sort: 'title', order: 'asc' } })
    return res.items ?? []
  }

  return { ...page, get, create, update, remove, effective, search }
})
