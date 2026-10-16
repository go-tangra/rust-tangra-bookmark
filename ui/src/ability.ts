import { ABILITY_TOKEN } from '@casl/vue'
import type { AnyAbility } from '@casl/ability'
import { inject } from 'vue'

/**
 * The shell's CASL ability (rules from the manifest's abilities, src/manifest.rs):
 * Bookmark read/create/update/delete, BookmarkPermission manage, BookmarkBackup
 * manage. Standalone (no ability provided) everything is allowed and the
 * server decides.
 */
export function useCan(): (action: string, subject: string) => boolean {
  const ability = inject<AnyAbility | null>(ABILITY_TOKEN, null)
  return (action, subject) => (ability ? ability.can(action, subject) : true)
}
