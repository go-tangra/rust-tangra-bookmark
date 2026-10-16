// The federated remote against the module manifest (src/manifest.rs): the
// exposes, the nav paths each served by a route, nav icons in the kit set;
// ./boot wiring the shell's bus; and a tiny host that mounts ./routes and
// renders every page.
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import { createRouter, createMemoryHistory, RouterView } from 'vue-router'
import { ICONS, UiToast, useToast } from '@go-tangra/ui'
import manifestRs from '../../../src/manifest.rs?raw'
import mfConfig from '../../module-federation.config.ts?raw'
import { routes } from '@/remote/routes'
import { nav } from '@/remote/nav'
import boot from '@/remote/boot'
import { setPlatformBus } from '@/stores/live'
import { axeViolations, bm, directory, fetchMock, grantRow, page, FakeSource } from './helpers'

const exposes = /EXPOSES: \[&str; \d+\] = \[([^\]]*)\]/.exec(manifestRs)![1]!.match(/"([^"]+)"/g)!.map((s) => s.slice(1, -1))
const navEntries = [...manifestRs.matchAll(/path: "([^"]+)"\.into\(\),\s*icon: "([^"]+)"\.into\(\)/g)].map((m) => ({ path: m[1]!, icon: m[2]! }))

describe('remote and manifest', () => {
  it('exposes exactly the manifest exposes', () => {
    // The config reads process.env (node), so the test reads its text.
    const block = /exposes: \{([^}]*)\}/.exec(mfConfig)![1]!
    expect([...block.matchAll(/'(\.\/[a-z]+)':/g)].map((m) => m[1]).sort()).toEqual([...exposes].sort())
    expect(mfConfig).toContain("name: 'bookmark'")
  })
  it('every manifest nav path is a route and every nav icon is in the kit set', () => {
    expect(navEntries.map((n) => n.path)).toEqual(['/bookmark', '/bookmark/permissions', '/bookmark/backup'])
    const paths = routes.map((r) => r.path)
    for (const n of navEntries) {
      expect(paths, n.path).toContain(n.path)
      expect(ICONS as readonly string[], n.icon).toContain(n.icon)
    }
    expect(nav()).toEqual([])
  })
})

describe('boot', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    useToast().clear()
  })
  it('keeps the shell bus and toasts bookmark.shared', async () => {
    const handlers: Record<string, (d: unknown) => void> = {}
    const bus = { on: vi.fn((t: string, fn: (d: unknown) => void) => ((handlers[t] = fn), () => delete handlers[t])) }
    boot({ live: bus })
    expect(bus.on).toHaveBeenCalledWith('bookmark.shared', expect.any(Function))
    handlers['bookmark.shared']!({ bookmark_id: 'b1', relation: 'editor' })
    handlers['bookmark.shared']!({}) // malformed: ignored
    expect(useToast().items.map((t) => [t.title, t.text])).toEqual([['A bookmark was shared with you', 'You can now open and change it under Bookmarks.']])
    setPlatformBus(null)
  })
  it('without a bus opens nothing', () => {
    const es = vi.fn()
    vi.stubGlobal('EventSource', es)
    boot({})
    expect(es).not.toHaveBeenCalled()
  })
})

describe('host harness: every route renders', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    document.cookie = '__Host-csrf=tok; Secure; Path=/'
    vi.stubGlobal('EventSource', FakeSource)
    document.body.innerHTML = ''
  })
  const cases: [string, string, string][] = [
    ['/bookmark', 'Bookmarks', 'Go docs'],
    ['/bookmark/permissions', 'Sharing', 'Maria'],
    ['/bookmark/backup', 'Bookmark backup', 'Export'],
  ]
  for (const [path, title, text] of cases) {
    it(path, async () => {
      fetchMock((url) => directory(url) ?? (url.includes('/permissions/effective') ? { permissions: { read: true, write: true, delete: true, share: true }, admin: false, grants: [] } : url.includes('/permissions') ? page([grantRow()]) : url.includes('/bookmarks/b1') ? bm() : page([bm()])))
      const router = createRouter({ history: createMemoryHistory(), routes })
      await router.push(path)
      await router.isReady()
      const Host = defineComponent({ render: () => h('div', [h(RouterView), h(UiToast)]) })
      const w = mount(Host, { global: { plugins: [router] }, attachTo: document.body })
      await flushPromises()
      await vi.dynamicImportSettled()
      await flushPromises()
      expect(w.find('h1').text()).toBe(title)
      expect(w.text()).toContain(text)
      expect(await axeViolations(w.element)).toEqual([])
      w.unmount()
    })
  }
})
