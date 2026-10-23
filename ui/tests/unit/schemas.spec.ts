import { describe, expect, it } from 'vitest'
import { bookmarkSchema, grantSchema, parseSubject } from '@/schemas'
import { fieldMessage, grantable, heldRelation } from '@/api/types'
import { withRepeated } from '@/stores/paged'
import { sharedNotice } from '@/stores/live'
import { ALL, READ_ONLY } from './helpers'

describe('bookmark form schema', () => {
  it('needs an http(s) address; trims; collapses duplicate tags; at most 20', () => {
    expect(bookmarkSchema.safeParse({ url: '' }).success).toBe(false)
    expect(bookmarkSchema.safeParse({ url: 'javascript:alert(1)' }).success).toBe(false)
    expect(bookmarkSchema.safeParse({ url: 'ftp://x.org' }).success).toBe(false)
    expect(bookmarkSchema.safeParse({ url: 'https://' }).success).toBe(false)
    expect(bookmarkSchema.parse({ url: ' https://go.dev ', title: ' Go ', tags: ['a', ' a ', 'b'] })).toEqual({ url: 'https://go.dev', title: 'Go', description: '', tags: ['a', 'b'] })
    expect(bookmarkSchema.parse({ url: 'HTTP://x.org', tags: null }).tags).toEqual([])
    expect(bookmarkSchema.safeParse({ url: 'https://x.org', tags: Array.from({ length: 21 }, (_, i) => 't' + i) }).success).toBe(false)
    expect(bookmarkSchema.safeParse({ url: 'https://x.org', tags: ['x'.repeat(65)] }).success).toBe(false)
    expect(bookmarkSchema.safeParse({ url: 'https://x.org/' + 'a'.repeat(2048) }).success).toBe(false)
  })
})

describe('share form schema', () => {
  it('a bookmark unless everyone; expiry in the future', () => {
    expect(grantSchema.safeParse({ subject: '', relation: 'viewer' }).success).toBe(false)
    expect(grantSchema.safeParse({ subject: 'user:u1', relation: 'viewer' }).success).toBe(false)
    expect(grantSchema.safeParse({ subject: 'tenant', relation: 'viewer' }).success).toBe(true)
    expect(grantSchema.safeParse({ subject: 'role:member', resource_id: 'b1', relation: 'owner' }).success).toBe(true)
    expect(grantSchema.safeParse({ subject: 'tenant', relation: 'boss' }).success).toBe(false)
    expect(grantSchema.safeParse({ subject: 'tenant', relation: 'viewer', expires: '2000-01-01' }).success).toBe(false)
    expect(grantSchema.safeParse({ subject: 'tenant', relation: 'viewer', expires: '2999-01-01' }).success).toBe(true)
  })
  it('parses subject keys', () => {
    expect(parseSubject('tenant')).toEqual({ subject_type: 'tenant', subject_id: '' })
    expect(parseSubject('role:member')).toEqual({ subject_type: 'role', subject_id: 'member' })
    expect(parseSubject('user:0190-a:b')).toEqual({ subject_type: 'user', subject_id: '0190-a:b' })
  })
})

describe('relations', () => {
  it('owners grant every level, sharers viewer and sharer, others nothing', () => {
    expect(grantable('owner')).toEqual(['viewer', 'sharer', 'editor', 'owner'])
    expect(grantable('sharer')).toEqual(['viewer', 'sharer'])
    expect(grantable('editor')).toEqual([])
    expect(grantable('')).toEqual([])
  })
  it('the held relation: admin owner, else the named one, else from the flags', () => {
    expect(heldRelation({ permissions: READ_ONLY, admin: true })).toBe('owner')
    expect(heldRelation({ permissions: ALL, admin: false, relation: 'editor' })).toBe('editor')
    expect(heldRelation({ permissions: ALL, admin: false, relation: null })).toBe('owner')
    expect(heldRelation({ permissions: { ...READ_ONLY, share: true }, admin: false })).toBe('sharer')
    expect(heldRelation({ permissions: { ...READ_ONLY, write: true }, admin: false })).toBe('editor')
    expect(heldRelation({ permissions: READ_ONLY, admin: false })).toBe('viewer')
    expect(heldRelation({ permissions: { ...READ_ONLY, read: false }, admin: false })).toBe('')
    expect(heldRelation(null)).toBe('')
  })
  it('names the server field codes', () => {
    expect(fieldMessage('scheme')).toBe('Must start with http:// or https://')
    expect(fieldMessage('past')).toBe('Must be in the future')
    expect(fieldMessage('weird')).toBe('Not valid')
  })
})

describe('list query and events', () => {
  it('sends list filters as repeated parameters', () => {
    expect(withRepeated('bookmarks', { tag: ['a', 'b c', ''], q: 'x' })).toEqual({ path: 'bookmarks?tag=a&tag=b+c', query: { q: 'x' } })
    expect(withRepeated('bookmarks', { tag: undefined })).toEqual({ path: 'bookmarks', query: { tag: undefined } })
  })
  it('turns bookmark.shared into a notice without content', () => {
    expect(sharedNotice({ bookmark_id: 'b1', relation: 'viewer' })).toEqual({ title: 'A bookmark was shared with you', text: 'You can now open it under Bookmarks.' })
    expect(sharedNotice({ relation: 'viewer' })).toBeNull()
    expect(sharedNotice('x')).toBeNull()
  })
})
