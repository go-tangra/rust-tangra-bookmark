import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import { ABILITY_TOKEN } from '@casl/vue'
import { createMongoAbility } from '@casl/ability'
import { createRouter, createMemoryHistory } from 'vue-router'
import Bookmarks from '@/views/bookmarks/index.vue'
import { setPlatformBus } from '@/stores/live'
import { ALL, READ_ONLY, answer, addChip, bm, body, buttonByText, callsTo, click, directory, fetchMock, FakeSource, mkRouter, page, panel, params, refuse, type } from './helpers'

const lists = (calls: { url: string; init: RequestInit }[]) => calls.filter((c) => c.init.method === 'GET' && c.url.startsWith('/api/bookmark/v1/bookmarks?'))
const effective = { permissions: ALL, relation: 'owner', admin: false, grants: [{ id: 'g0', resource_type: 'bookmark', resource_id: 'b1', subject_type: 'role', subject_id: 'member', relation: 'owner', granted_by: 'u1', granted_at: '2026-01-01T00:00:00Z' }] }

function server(rows = [bm(), bm({ id: 'b2', title: 'Shared', url: 'https://example.org', tags: [], permissions: READ_ONLY })]) {
  return (url: string, init: RequestInit) => {
    const d = directory(url)
    if (d) return d
    if (url.includes('/permissions/effective')) return effective
    if (init.method === 'POST' && url.endsWith('/bookmarks')) return bm({ id: 'b3', title: 'New' })
    if (init.method === 'PUT') return { ...rows[0], ...JSON.parse(String(init.body)) }
    if (url.endsWith('/remove')) return 204
    return page(rows, { total: 2 })
  }
}

async function mountWith(rules?: { action: string; subject: string }[], router = mkRouter()) {
  const provide = rules ? { [ABILITY_TOKEN as symbol]: createMongoAbility(rules) } : {}
  const w = mount(Bookmarks, { global: { plugins: [router], provide }, attachTo: document.body })
  await flushPromises()
  return w
}

describe('bookmarks page', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    document.cookie = '__Host-csrf=tok; Secure; Path=/'
    document.body.innerHTML = ''
    vi.stubGlobal('EventSource', FakeSource)
    ;(globalThis as unknown as { __vw: number }).__vw = 1280
  })
  afterEach(() => vi.useRealTimers())

  it('lists the first page newest first with links that open in a new tab', async () => {
    const calls = fetchMock(server())
    const w = await mountWith()
    expect(lists(calls)[0]!.url).toBe('/api/bookmark/v1/bookmarks?page=1&page_size=25&sort=created_at&order=desc')
    expect(w.find('[data-test="bookmark-row-b1"]').text()).toContain('Go docs')
    const open = w.find('[data-test="bookmark-open-b1"]')
    expect([open.attributes('href'), open.attributes('target'), open.attributes('rel')]).toEqual(['https://go.dev/doc', '_blank', 'noopener noreferrer'])
    // Every sortable column is a server sort field.
    const headers = w.findAll('th button').map((b) => b.text())
    for (const l of ['Title', 'Address', 'Changed', 'Created']) expect(headers.some((t) => t.startsWith(l)), l).toBe(true)
    await w.findAll('th button').find((b) => b.text().startsWith('Title'))!.trigger('click')
    await flushPromises()
    expect([params(lists(calls).at(-1)!.url).get('sort'), params(lists(calls).at(-1)!.url).get('order')]).toEqual(['title', 'asc'])
    w.unmount()
  })

  it('search (debounced) and tags filter on the server; a tag chip adds itself; back to page 1', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    const r = createRouter({ history: createMemoryHistory(), routes: [{ path: '/bookmark', component: Bookmarks }] })
    await r.push('/bookmark?bookmarks.page=2')
    const calls = fetchMock(server())
    const w = await mountWith(undefined, r)
    expect(params(lists(calls)[0]!.url).get('page')).toBe('2')
    await type(document.querySelector('#bookmark-search'), 'go')
    await type(document.querySelector('#bookmark-search'), 'go d')
    vi.advanceTimersByTime(350)
    await flushPromises()
    const last = () => params(lists(calls).at(-1)!.url)
    expect([last().get('q'), last().get('page')]).toEqual(['go d', '1'])
    expect(lists(calls).filter((c) => params(c.url).get('q') === 'go').length).toBe(0) // debounced away
    await addChip(document.querySelector('#bookmark-tags'), 'docs')
    await click(w.find('[data-test="bookmark-row-b1"]').findAll('button').find((b) => b.text() === 'go')!.element)
    expect(last().getAll('tag')).toEqual(['docs', 'go'])
    expect(last().get('q')).toBe('go d')
    w.unmount()
  })

  it('row actions follow the row permissions and the abilities', async () => {
    fetchMock(server())
    const w = await mountWith()
    for (const a of ['edit', 'share', 'remove']) {
      expect(w.find(`[data-test="bookmark-${a}-b1"]`).exists(), a).toBe(true)
      expect(w.find(`[data-test="bookmark-${a}-b2"]`).exists(), a + ' on a viewer row').toBe(false)
    }
    expect(w.find('[data-test=bookmark-new]').exists()).toBe(true)
    w.unmount()
    // A bookmark viewer (bookmarks:read only): no create, edit, share or remove anywhere.
    const v = await mountWith([{ action: 'read', subject: 'Bookmark' }])
    expect(v.find('[data-test=bookmark-new]').exists()).toBe(false)
    for (const a of ['edit', 'share', 'remove']) expect(v.find(`[data-test="bookmark-${a}-b1"]`).exists(), a).toBe(false)
    expect(v.find('[data-test="bookmark-open-b1"]').exists()).toBe(true)
    v.unmount()
    // Sharing needs permissions:manage on top of share on the row.
    const m = await mountWith([{ action: 'read', subject: 'Bookmark' }, { action: 'update', subject: 'Bookmark' }])
    expect(m.find('[data-test="bookmark-edit-b1"]').exists()).toBe(true)
    expect(m.find('[data-test="bookmark-share-b1"]').exists()).toBe(false)
    m.unmount()
  })

  it('creates a bookmark: validated client side, server field codes in words, then posted and the page reloaded', async () => {
    let refuseOnce = true
    const calls = fetchMock((url, init) => {
      if (init.method === 'POST' && url.endsWith('/bookmarks') && refuseOnce) {
        refuseOnce = false
        return refuse(422, { reason: 'validation_failed', detail: { fields: { url: 'scheme', 'tags.1': 'too_long' } } })
      }
      return server()(url, init)
    })
    const w = await mountWith()
    await w.find('[data-test=bookmark-new]').trigger('click')
    await flushPromises()
    const form = panel('bookmark-form')!
    await click(form.querySelector('[data-test=bookmark-save]'))
    expect(callsTo(calls, 'POST', '/bookmarks').length).toBe(0)
    expect(form.textContent).toContain('Required')
    await type(form.querySelector('input[data-field=url]'), 'ftp://x')
    await click(form.querySelector('[data-test=bookmark-save]'))
    expect(form.textContent).toContain('Must start with http:// or https://')
    expect(callsTo(calls, 'POST', '/bookmarks').length).toBe(0)
    await type(form.querySelector('input[data-field=url]'), 'https://vuejs.org')
    await type(form.querySelector('input[data-field=title]'), ' Vue ')
    await addChip(form.querySelector('#tags'), 'js')
    await addChip(form.querySelector('#tags'), 'js')
    await click(form.querySelector('[data-test=bookmark-save]'))
    expect(form.textContent).toContain('Must start with http:// or https://') // the server's code, in words
    expect(form.textContent).toContain('Too long')
    await click(form.querySelector('[data-test=bookmark-save]'))
    expect(body(calls, 'POST', '/bookmarks')).toEqual({ url: 'https://vuejs.org', title: 'Vue', description: '', tags: ['js'] })
    expect((callsTo(calls, 'POST', '/bookmarks').at(-1)!.init.headers as Record<string, string>)['X-CSRF-Token']).toBe('tok')
    expect(panel('bookmark-form')).toBeNull()
    expect(lists(calls).length).toBe(2)
    w.unmount()
  })

  it('edits from the row: the form starts from the bookmark and PUTs every field', async () => {
    const calls = fetchMock(server())
    const w = await mountWith()
    await w.find('[data-test="bookmark-edit-b1"]').trigger('click')
    await flushPromises()
    const form = panel('bookmark-form')!
    expect(form.querySelector<HTMLInputElement>('input[data-field=url]')!.value).toBe('https://go.dev/doc')
    expect(form.textContent).toContain('docs')
    await type(form.querySelector('input[data-field=title]'), 'Go documentation')
    await click(buttonByText(form, 'Remove go') ?? form.querySelector('button[aria-label="Remove go"]'))
    await click(form.querySelector('[data-test=bookmark-save]'))
    expect(callsTo(calls, 'PUT', '/bookmarks/b1').length).toBe(1)
    expect(body(calls, 'PUT', '/bookmarks/b1')).toEqual({ url: 'https://go.dev/doc', title: 'Go documentation', description: 'The Go documentation', tags: ['docs'] })
    w.unmount()
  })

  it('removes after confirmation only', async () => {
    const calls = fetchMock(server())
    const w = await mountWith()
    await w.find('[data-test="bookmark-remove-b1"]').trigger('click')
    await answer(false)
    expect(callsTo(calls, 'POST', '/bookmarks/b1/remove').length).toBe(0)
    await w.find('[data-test="bookmark-remove-b1"]').trigger('click')
    await answer(true)
    expect(callsTo(calls, 'POST', '/bookmarks/b1/remove').length).toBe(1)
    expect(lists(calls).length).toBe(2)
    w.unmount()
  })

  it('a row opens the bookmark with the caller access and the shares giving it', async () => {
    const calls = fetchMock(server())
    const w = await mountWith()
    await w.find('[data-test="bookmark-row-b1"]').trigger('click')
    await flushPromises()
    const view = panel('bookmark-view')!
    expect(params(calls.find((c) => c.url.includes('/permissions/effective'))!.url).get('resource_id')).toBe('b1')
    expect(view.textContent).toContain('The Go documentation')
    expect(view.textContent).toContain('Ana') // created_by resolved through auth
    expect(view.querySelector('[data-test=bookmark-allowed]')!.textContent).toContain('open, change, remove, share')
    expect(view.querySelector('[data-test=bookmark-via]')!.textContent).toContain('owner for Role: Member')
    await click(view.querySelector('[data-test=view-share]'))
    expect(panel('bookmark-view')).toBeNull()
    expect(document.body.textContent).toContain('Share Go docs')
    w.unmount()
  })

  it('reloads on bookmark.shared (shell bus, else its own stream)', async () => {
    const calls = fetchMock(server())
    const w = await mountWith()
    expect(FakeSource.last!.url).toBe('/gateway/v1/stream')
    FakeSource.last!.emit('bookmark.shared', { bookmark_id: 'b9', relation: 'viewer' })
    await flushPromises()
    expect(lists(calls).length).toBe(2)
    w.unmount()
    const handlers: Record<string, (d: unknown) => void> = {}
    setPlatformBus({ on: (t, fn) => ((handlers[t] = fn), () => delete handlers[t]) })
    const calls2 = fetchMock(server())
    const w2 = await mountWith()
    handlers['bookmark.shared']!({ bookmark_id: 'b9', relation: 'viewer' })
    await flushPromises()
    expect(lists(calls2).length).toBe(2)
    w2.unmount()
    expect(handlers['bookmark.shared']).toBeUndefined()
    setPlatformBus(null)
  })
})
