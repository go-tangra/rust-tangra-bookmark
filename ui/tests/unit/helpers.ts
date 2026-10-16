// Shared fixtures of the bookmark UI unit tests: a fetch mock that records
// calls, sample rows, a router and small DOM helpers (the kit drawers and
// dialogs teleport to document.body).
import { vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createRouter, createMemoryHistory } from 'vue-router'
import { axe } from 'vitest-axe'
import { useConfirm } from '@go-tangra/ui'
import type { Bookmark, Grant } from '@/api/types'

export type Call = { url: string; init: RequestInit }
export type Handler = (url: string, init: RequestInit) => unknown

/** Responds with handler's value: a Response as is, 204, or JSON 200. */
export function fetchMock(handler: Handler): Call[] {
  const calls: Call[] = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit = {}) => {
    calls.push({ url, init })
    const body = handler(url, init)
    if (body === 204) return new Response(null, { status: 204 })
    if (body instanceof Response) return body
    return new Response(JSON.stringify(body ?? {}), { status: 200, headers: { 'Content-Type': 'application/json' } })
  }))
  return calls
}

/** A JSON refusal. */
export const refuse = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

export const page = <T>(items: T[], extra: Record<string, unknown> = {}) => ({ items, total: items.length, page: 1, page_size: 25, sort: 'created_at', order: 'desc', ...extra })

export const ALL = { read: true, write: true, delete: true, share: true }
export const READ_ONLY = { read: true, write: false, delete: false, share: false }

export const bm = (over: Partial<Bookmark> = {}): Bookmark => ({
  id: 'b1',
  url: 'https://go.dev/doc',
  title: 'Go docs',
  description: 'The Go documentation',
  tags: ['go', 'docs'],
  created_by: 'u1',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-02T00:00:00Z',
  permissions: ALL,
  ...over,
})

export const grantRow = (over: Partial<Grant> = {}): Grant => ({
  id: 'g1',
  resource_type: 'bookmark',
  resource_id: 'b1',
  subject_type: 'user',
  subject_id: 'u2',
  relation: 'viewer',
  granted_by: 'u1',
  granted_at: '2026-01-03T00:00:00Z',
  ...over,
})

/** Auth directory answers (POST /api/v1/users/lookup, GET /api/v1/users, GET /api/v1/roles). */
export function directory(url: string): unknown {
  if (url.includes('/api/v1/users/lookup')) return { items: [{ id: 'u1', display_name: 'Ana' }, { id: 'u2', display_name: 'Maria' }] }
  if (url.startsWith('/api/v1/users')) return { items: [{ id: 'u3', display_name: 'Petar' }] }
  if (url.includes('/api/v1/roles')) return [{ slug: 'member', display_name: 'Member' }]
  return undefined
}

export class FakeSource {
  static last: FakeSource | null = null
  onopen = null
  onerror = null
  listeners: Record<string, (e: MessageEvent) => void> = {}
  constructor(public url: string) {
    FakeSource.last = this
  }
  addEventListener(t: string, fn: (e: MessageEvent) => void) {
    this.listeners[t] = fn
  }
  emit(t: string, data: unknown) {
    this.listeners[t]?.({ data: JSON.stringify(data) } as MessageEvent)
  }
  close() {}
}

export const mkRouter = () => createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { template: '<div/>' } }] })

/** The open drawer panel with this data-test (kit drawers are <aside role="dialog">). */
export const panel = (test: string) => document.body.querySelector<HTMLElement>(`aside[role=dialog][data-test="${test}"]`)

export async function type(el: Element | null, value: string): Promise<void> {
  if (!el) throw new Error('no input')
  const i = el as HTMLInputElement
  i.value = value
  i.dispatchEvent(new Event('input'))
  await flushPromises()
}

/** Types a tag into a chip input and presses Enter. */
export async function addChip(el: Element | null, value: string): Promise<void> {
  await type(el, value)
  el!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }))
  await flushPromises()
}

export async function click(el: Element | null): Promise<void> {
  if (!el) throw new Error('no element')
  ;(el as HTMLElement).click()
  await flushPromises()
}

export const buttonByText = (root: ParentNode, text: string) => Array.from(root.querySelectorAll('button')).find((b) => b.textContent?.trim() === text) ?? null

/** The calls with this method whose path (without the query) ends with suffix. */
export const callsTo = (calls: Call[], method: string, suffix: string) => calls.filter((c) => (c.init.method ?? 'GET') === method && c.url.split('?')[0]!.endsWith(suffix))

/** The JSON body of the last such call. */
export function body(calls: Call[], method: string, suffix: string): Record<string, unknown> | undefined {
  const c = callsTo(calls, method, suffix).at(-1)
  return c ? (JSON.parse(String(c.init.body)) as Record<string, unknown>) : undefined
}

export const params = (url: string) => new URL(url, 'https://x').searchParams

/** Answers the pending confirm dialog (the shell hosts UiConfirm). */
export async function answer(ok: boolean): Promise<void> {
  useConfirm().answer(ok)
  await flushPromises()
}

/** Types into a kit combobox and chooses the option containing `title`. */
export async function choose(id: string, q: string, title: string): Promise<void> {
  const input = document.querySelector<HTMLInputElement>('#' + id)
  if (!input) throw new Error('no combobox ' + id)
  input.dispatchEvent(new Event('focus'))
  await type(input, q)
  const opt = Array.from(document.querySelectorAll<HTMLElement>('#' + id + '-listbox [role=option] button')).find((o) => o.textContent?.includes(title))
  if (!opt) throw new Error('no option ' + title + ' in ' + id)
  opt.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  await flushPromises()
}

/** Sets a native select. */
export async function pick(el: Element | null, value: string): Promise<void> {
  if (!el) throw new Error('no select')
  ;(el as HTMLSelectElement).value = value
  el.dispatchEvent(new Event('change'))
  await flushPromises()
}

/** axe violations in jsdom (colour contrast and landmarks are the shell's). */
export async function axeViolations(el: Element): Promise<string[]> {
  const res = await axe(el as HTMLElement, { rules: { 'color-contrast': { enabled: false }, region: { enabled: false } } })
  return res.violations.map((v) => v.id + ': ' + v.nodes.map((n) => n.target.join(' ')).join(', '))
}
