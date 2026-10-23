import { setPlatformBus, toastShares, type PlatformBus } from '@/stores/live'

// The shell calls ./boot once the module is mounted (contracts/federation.md)
// with its BootContext. The module keeps the shared realtime bus so
// bookmark.shared rides the shell's single platform stream, and toasts every
// share wherever the person is in the portal. Without the bus (an older shell)
// nothing is opened here: the bookmark list still reloads on the event.
export default function boot(ctx: { live?: PlatformBus } = {}): void {
  setPlatformBus(ctx.live)
  if (ctx.live) toastShares()
}
