import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ListQueryOptions } from '@go-tangra/ui'
import { api, describe } from '@/api/client'
import type { Effective, Grant, GrantPage, NewGrant } from '@/api/types'
import { pagedList } from '@/stores/paged'

/** Sortable fields of GET /permissions (default granted_at desc). */
export const GRANT_SORTS = ['granted_at', 'relation', 'subject_type'] as const
export const GRANT_LIST: ListQueryOptions = { sortable: [...GRANT_SORTS], defaultSort: { key: 'granted_at', dir: 'desc' }, defaultSize: 25 }

export interface GrantFilter extends Record<string, string | undefined> {
  resource_id?: string | undefined
  subject_type?: string | undefined
  subject_id?: string | undefined
}

const grant = (input: NewGrant) => api<Grant>('POST', 'permissions/grant', { ...input, resource_type: 'bookmark' })
const revoke = (resourceId: string, id: string) => api<{ revoked: boolean }>('POST', 'permissions/revoke', { resource_type: 'bookmark', resource_id: resourceId, id })

/** Every grant of the tenant the caller may share: the Sharing page. */
export const useGrants = defineStore('bookmark-grants', () => {
  const page = pagedList<Grant, GrantFilter>('permissions', { page: 1, page_size: 25, sort: 'granted_at', order: 'desc' })
  async function remove(g: Grant): Promise<void> {
    await revoke(g.resource_id, g.id)
    page.items.value = page.items.value.filter((x) => x.id !== g.id)
  }
  return { ...page, grant, remove }
})

/** The grants of one bookmark and the caller's own access to it: the share drawer. */
export const useSharing = defineStore('bookmark-sharing', () => {
  const items = ref<Grant[]>([])
  const mine = ref<Effective | null>(null)
  const loading = ref(false)
  const error = ref('')
  let current = ''

  async function load(resourceId: string): Promise<void> {
    current = resourceId
    loading.value = true
    error.value = ''
    items.value = []
    mine.value = null
    try {
      // The caller's access first: the grant list needs share on the bookmark.
      const eff = await api<Effective>('GET', 'permissions/effective', undefined, { query: { resource_id: resourceId } })
      if (current !== resourceId) return
      mine.value = eff
      if (!eff.permissions.share) return
      const res = await api<GrantPage>('GET', 'permissions', undefined, { query: { resource_id: resourceId, page_size: 200 } })
      if (current === resourceId) items.value = res.items ?? []
    } catch (e) {
      if (current === resourceId) error.value = describe(e)
    } finally {
      if (current === resourceId) loading.value = false
    }
  }

  async function add(input: NewGrant): Promise<Grant> {
    const g = await grant(input)
    items.value = [g, ...items.value]
    return g
  }

  async function remove(id: string): Promise<void> {
    await revoke(current, id)
    items.value = items.value.filter((x) => x.id !== id)
  }

  return { items, mine, loading, error, load, add, remove }
})
