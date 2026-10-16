// Domain types from the OpenAPI contract (api/openapi/bookmark.yaml, generated
// into schema.d.ts by `npm run gen:api`) plus the relation helpers the share
// drawer needs.
import type { components } from './schema.d'

type S = components['schemas']

export type Bookmark = S['Bookmark']
export type BookmarkPage = S['BookmarkPage']
export type NewBookmark = S['NewBookmark']
export type BookmarkPatch = S['BookmarkPatch']
export type Permissions = S['Permissions']
export type Relation = S['Relation']
export type SubjectType = S['SubjectType']
export type Grant = S['Grant']
export type GrantPage = S['GrantPage']
/** A new grant (resource_type is always bookmark; the store sets it). */
export type NewGrant = Omit<S['NewGrant'], 'resource_type'>
export type Effective = S['Effective']
export type Backup = S['Backup']
export type ImportResult = S['ImportResult']
export type ImportMode = NonNullable<S['ImportRequest']['mode']>

/** The list contract's page parameters (go-tangra specs/032-server-side-tables). */
export interface ListParams {
  page: number
  page_size: number
  sort: string
  order: 'asc' | 'desc'
}

// Auth directory hits (public profiles of the caller's tenant; role list).
export interface UserHit {
  id: string
  display_name: string
  avatar_url?: string
  email?: string
}

export interface RoleHit {
  slug: string
  display_name: string
}

export const RELATIONS: Relation[] = ['viewer', 'sharer', 'editor', 'owner']

/** What each relation allows (the help page's table). */
export const RELATION_TEXT: Record<Relation, string> = {
  owner: 'open, change, remove and share',
  editor: 'open and change',
  sharer: 'open and share',
  viewer: 'open',
}

/**
 * Relations the holder of `held` may grant, lowest first: owners every level,
 * sharers viewer and sharer (they cannot make anyone an editor or owner).
 */
export function grantable(held: string): Relation[] {
  if (held === 'owner') return [...RELATIONS]
  if (held === 'sharer') return ['viewer', 'sharer']
  return []
}

/**
 * The caller's relation: the one the server names, owner for tenant admins,
 * else reconstructed from the effective flags.
 */
export function heldRelation(e: Pick<Effective, 'permissions' | 'admin'> & { relation?: Relation | null | undefined } | null | undefined): Relation | '' {
  if (!e) return ''
  if (e.admin) return 'owner'
  if (e.relation) return e.relation
  const p = e.permissions
  if (p.read && p.write && p.delete && p.share) return 'owner'
  if (p.share) return 'sharer'
  if (p.write) return 'editor'
  if (p.read) return 'viewer'
  return ''
}

/** Badge colours per relation (v3: owner red, editor orange, sharer purple, viewer blue). */
export const RELATION_COLORS = { owner: 'error', editor: 'warning', sharer: 'secondary', viewer: 'info' } as const

/** The server's field codes (FieldErrors) in words. */
const FIELD_CODES: Record<string, string> = {
  required: 'Required',
  invalid: 'Not valid',
  too_long: 'Too long',
  too_many: 'Too many',
  unknown: 'Not known',
  scheme: 'Must start with http:// or https://',
  past: 'Must be in the future',
}

export function fieldMessage(code: unknown): string {
  return (typeof code === 'string' && FIELD_CODES[code]) || 'Not valid'
}
