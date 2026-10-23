import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import { useConfirm } from '@go-tangra/ui'
import Sharing from '@/views/permissions/index.vue'
import { ALL, answer, bm, body, callsTo, choose, click, directory, fetchMock, grantRow, mkRouter, page, panel, params, refuse, type } from './helpers'

const rows = [
  grantRow(),
  grantRow({ id: 'g2', subject_type: 'role', subject_id: 'member', relation: 'editor', expires_at: '2000-01-01T00:00:00Z' }),
  grantRow({ id: 'g3', resource_id: '', subject_type: 'tenant', subject_id: '', relation: 'viewer' }),
]

function server(admin = true, onGrant?: (b: Record<string, unknown>) => unknown) {
  return (url: string, init: RequestInit) => {
    const d = directory(url)
    if (d) return d
    if (url.includes('/permissions/effective')) return { permissions: ALL, relation: admin ? 'owner' : null, admin, grants: [] }
    if (url.includes('/permissions/grant')) return onGrant?.(JSON.parse(String(init.body))) ?? grantRow({ id: 'g9' })
    if (url.includes('/permissions/revoke')) return { revoked: true }
    if (url.startsWith('/api/bookmark/v1/permissions')) return page(rows, { sort: 'granted_at' })
    if (url.includes('/bookmarks/b1')) return bm()
    return page([bm(), bm({ id: 'b2', title: 'Rust book', url: 'https://doc.rust-lang.org' })])
  }
}

const mountPage = async () => {
  const w = mount(Sharing, { global: { plugins: [mkRouter()] }, attachTo: document.body })
  await flushPromises()
  return w
}
const grantLists = (calls: { url: string; init: RequestInit }[]) => calls.filter((c) => c.url.startsWith('/api/bookmark/v1/permissions?'))

describe('sharing page', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    document.cookie = '__Host-csrf=tok; Secure; Path=/'
    document.body.innerHTML = ''
    ;(globalThis as unknown as { __vw: number }).__vw = 1280
  })

  it('lists every share with the bookmarks, subjects and granting users named', async () => {
    const calls = fetchMock(server())
    const w = await mountPage()
    expect(grantLists(calls)[0]!.url).toBe('/api/bookmark/v1/permissions?page=1&page_size=25&sort=granted_at&order=desc')
    const r1 = w.find('[data-test="grant-row-g1"]').text()
    expect(r1).toContain('Go docs') // the bookmark title, by ID
    expect(r1).toContain('Maria') // the grantee
    expect(r1).toContain('Ana') // granted by
    expect(r1).toContain('Never')
    const r2 = w.find('[data-test="grant-row-g2"]').text()
    expect(r2).toContain('Member')
    expect(r2).toContain('expired')
    const r3 = w.find('[data-test="grant-row-g3"]').text()
    expect(r3).toContain('Every bookmark')
    expect(r3).toContain('Everyone in the tenant')
    expect(callsTo(calls, 'GET', '/bookmarks/b1').length).toBe(1) // one lookup per bookmark
    w.unmount()
  })

  it('filters by subject and by bookmark on the server', async () => {
    const calls = fetchMock(server())
    const w = await mountPage()
    await choose('grant-filter-subject', 'Mem', 'Role: Member')
    const last = () => params(grantLists(calls).at(-1)!.url)
    expect([last().get('subject_type'), last().get('subject_id'), last().get('page')]).toEqual(['role', 'member', '1'])
    await choose('grant-filter-subject', 'Pe', 'Petar')
    expect([last().get('subject_type'), last().get('subject_id')]).toEqual(['user', 'u3'])
    await choose('grant-filter-bookmark', 'Rust', 'Rust book')
    expect([last().get('subject_id'), last().get('resource_id')]).toEqual(['u3', 'b2'])
    await choose('grant-filter-subject', 'Every', 'Everyone in the tenant')
    expect([last().get('subject_type'), last().has('subject_id')]).toEqual(['tenant', false])
    w.unmount()
  })

  it('removes a share after confirmation, naming its bookmark', async () => {
    const calls = fetchMock(server())
    const w = await mountPage()
    await w.find('[data-test="grant-remove-g3"]').trigger('click')
    await flushPromises()
    expect(useConfirm().state.pending?.text).toBe('Everyone in the tenant loses viewer access to Every bookmark.')
    await answer(true)
    expect(body(calls, 'POST', '/permissions/revoke')).toEqual({ resource_type: 'bookmark', resource_id: '', id: 'g3' })
    w.unmount()
  })

  it('an administrator shares every bookmark with everyone; a person needs a bookmark', async () => {
    const calls = fetchMock(server(true))
    const w = await mountPage()
    await w.find('[data-test=grant-new]').trigger('click')
    await flushPromises()
    const form = panel('grant-form')!
    expect(form.textContent).toContain('Bookmark (empty: every bookmark)')
    await choose('subject', 'Pe', 'Petar')
    await click(form.querySelector('[data-test=grant-save]'))
    expect(form.textContent).toContain('Every bookmark can be shared with everyone only')
    expect(callsTo(calls, 'POST', '/permissions/grant').length).toBe(0)
    await choose('subject', 'Every', 'Everyone in the tenant')
    await type(form.querySelector('input[data-field=expires]'), '2099-01-01')
    await click(form.querySelector('[data-test=grant-save]'))
    expect(body(calls, 'POST', '/permissions/grant')).toEqual({ subject_type: 'tenant', subject_id: '', relation: 'viewer', expires_at: '2099-01-01T00:00:00.000Z', resource_type: 'bookmark' })
    expect(panel('grant-form')).toBeNull()
    w.unmount()
  })

  it('a share that exists already is named on the form', async () => {
    fetchMock(server(false, () => refuse(409, { reason: 'conflict' })))
    const w = await mountPage()
    await w.find('[data-test=grant-new]').trigger('click')
    await flushPromises()
    const form = panel('grant-form')!
    expect(form.textContent).not.toContain('empty: every bookmark')
    await choose('subject', 'Pe', 'Petar')
    await choose('resource_id', 'Go', 'Go docs')
    await click(form.querySelector('[data-test=grant-save]'))
    expect(form.textContent).toContain('Already holds this level on the bookmark')
    w.unmount()
  })
})
