import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import SharingDrawer from '@/components/SharingDrawer.vue'
import { ALL, READ_ONLY, body, callsTo, choose, click, directory, fetchMock, grantRow, page, params, pick, type } from './helpers'

const eff = (permissions = ALL, relation: string | null = 'owner', admin = false) => ({ permissions, relation, admin, grants: [] })

function server(e = eff()) {
  return (url: string, init: RequestInit) =>
    directory(url) ??
    (url.includes('/permissions/effective')
      ? e
      : url.includes('/permissions/grant')
        ? grantRow({ id: 'g2', subject_type: 'role', subject_id: 'member', relation: 'editor', ...JSON.parse(String(init.body)) })
        : url.includes('/permissions/revoke')
          ? { revoked: true }
          : page([grantRow(), grantRow({ id: 'g3', subject_type: 'tenant', subject_id: '', relation: 'viewer', expires_at: '2099-01-01T00:00:00Z' })]))
}

const mountDrawer = async (props: Record<string, unknown> = {}) => {
  const w = mount(SharingDrawer, { props: { bookmarkId: 'b1', name: 'Go docs', modelValue: true, ...props }, attachTo: document.body })
  await flushPromises()
  return w
}

describe('share drawer', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    document.cookie = '__Host-csrf=tok; Secure; Path=/'
    document.body.innerHTML = ''
  })

  it('lists the grants of the bookmark with names from auth and the caller relation', async () => {
    const calls = fetchMock(server())
    await mountDrawer()
    const text = document.body.textContent ?? ''
    expect(text).toContain('Share Go docs')
    expect(text).toContain('Maria')
    expect(text).toContain('Everyone in the tenant')
    expect(text).toContain('Your relation: owner')
    const list = calls.find((c) => c.url.startsWith('/api/bookmark/v1/permissions?'))!
    expect([params(list.url).get('resource_id'), params(list.url).get('page_size')]).toEqual(['b1', '200'])
    expect(body(calls, 'POST', '/api/v1/users/lookup')).toEqual({ ids: ['u2'] })
    // An owner grants every level.
    expect(Array.from(document.querySelectorAll<HTMLOptionElement>('#perm-level option')).map((o) => o.value).filter(Boolean)).toEqual(['viewer', 'sharer', 'editor', 'owner'])
  })

  it('grants to a role with an expiry and revokes by grant id', async () => {
    const calls = fetchMock(server())
    const w = await mountDrawer()
    // Pick "Role: Member" in the subject combobox, editor, an expiry.
    await choose('perm-subject', 'Mem', 'Role: Member')
    await pick(document.querySelector('#perm-level'), 'editor')
    await type(document.querySelector('#perm-expires'), '2099-06-01')
    await click(Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.trim() === 'Grant')!)
    expect(body(calls, 'POST', '/permissions/grant')).toEqual({ resource_id: 'b1', subject_type: 'role', subject_id: 'member', relation: 'editor', expires_at: '2099-06-01T00:00:00.000Z', resource_type: 'bookmark' })
    expect(w.emitted('changed')).toHaveLength(1)
    await click(document.querySelector('button[aria-label="Revoke"]'))
    expect(body(calls, 'POST', '/permissions/revoke')).toEqual({ resource_type: 'bookmark', resource_id: 'b1', id: 'g2' })
    expect(w.emitted('changed')).toHaveLength(2)
  })

  it('a sharer passes on viewer and sharer only', async () => {
    fetchMock(server(eff({ read: true, write: false, delete: false, share: true }, 'sharer')))
    await mountDrawer()
    expect(document.body.textContent).toContain('Your relation: sharer')
    expect(Array.from(document.querySelectorAll<HTMLOptionElement>('#perm-level option')).map((o) => o.value).filter(Boolean)).toEqual(['viewer', 'sharer'])
  })

  it('a tenant admin holds owner', async () => {
    fetchMock(server(eff(ALL, null, true)))
    await mountDrawer()
    expect(document.body.textContent).toContain('Your relation: owner (administrator)')
  })

  it('without share the grants are not listed and nothing can be granted', async () => {
    const calls = fetchMock(server(eff(READ_ONLY, 'viewer')))
    await mountDrawer()
    expect(calls.some((c) => c.url.startsWith('/api/bookmark/v1/permissions?'))).toBe(false)
    expect(document.body.textContent).toContain('You need the share permission')
    expect(document.querySelector('button[aria-label="Revoke"]')).toBeNull()
  })

  it('does nothing while closed', async () => {
    const calls = fetchMock(server())
    await mountDrawer({ modelValue: false })
    expect(callsTo(calls, 'GET', '/permissions/effective').length).toBe(0)
  })
})
