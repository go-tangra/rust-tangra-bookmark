// bookmark.shared reaches the UI over the platform's realtime bus: the module
// publishes it to the grantee's user stream and the gateway relays the stream
// to the browser. Inside the shell the module listens through the shell's
// shared connection (BootContext.live, handed to ./boot); standalone it opens
// its own EventSource on the gateway's stream.
import { useToast } from '@go-tangra/ui'
import type { Relation } from '@/api/types'

/** The shell's shared realtime bus (BootContext.live). */
export interface PlatformBus {
  on(type: string, fn: (data: unknown) => void): () => void
}

/** The gateway's platform event stream. */
export const PLATFORM_STREAM = '/gateway/v1/stream'

/** A bookmark was shared with the signed-in user (contracts/audit-events.md). */
export const EVENT_SHARED = 'bookmark.shared'
export interface SharedEvent {
  bookmark_id: string
  relation: Relation
}

let platformBus: PlatformBus | null = null

/** Hands the shell's bus to the module (called by ./boot). */
export function setPlatformBus(bus: PlatformBus | null | undefined): void {
  platformBus = bus ?? null
}

/** Whether the shell's bus is in use. */
export const hasPlatformBus = () => platformBus !== null

/** Calls fn for every event of type; returns the unsubscribe function. */
export function onEvent(type: string, fn: (data: unknown) => void): () => void {
  if (platformBus) return platformBus.on(type, fn)
  if (typeof EventSource === 'undefined') return () => {}
  const source = new EventSource(PLATFORM_STREAM, { withCredentials: true })
  source.addEventListener(type, (e) => {
    try {
      fn(JSON.parse((e as MessageEvent).data))
    } catch {
      /* non-JSON payloads are ignored */
    }
  })
  return () => source.close()
}

/** The notice for a share event (IDs and relation only; no content). */
export function sharedNotice(data: unknown): { title: string; text: string } | null {
  const ev = data as Partial<SharedEvent> | null
  if (!ev || typeof ev !== 'object' || !ev.bookmark_id) return null
  return { title: 'A bookmark was shared with you', text: ev.relation ? 'You can now ' + verb(ev.relation) + ' it under Bookmarks.' : 'Find it under Bookmarks.' }
}

function verb(r: Relation): string {
  return { owner: 'open, change, remove and share', editor: 'open and change', sharer: 'open and share', viewer: 'open' }[r] ?? 'open'
}

let offToasts: (() => void) | null = null

/** Shows a toast for every bookmark.shared event (once per page load). */
export function toastShares(): void {
  offToasts?.()
  offToasts = onEvent(EVENT_SHARED, (data) => {
    const n = sharedNotice(data)
    if (n) useToast().info(n.title, n.text)
  })
}
