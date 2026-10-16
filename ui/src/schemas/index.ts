import { z } from 'zod'
import { RELATIONS } from '@/api/types'

const tag = z.string().trim().min(1).max(64, 'At most 64 characters')

/** Create and edit form of a bookmark (NewBookmark / BookmarkPatch). */
export const bookmarkSchema = z.object({
  url: z
    .string()
    .trim()
    .min(1, 'Required')
    .max(2048, 'At most 2048 characters')
    .refine((s) => /^https?:\/\/[^\s/?#]+/i.test(s), 'Must start with http:// or https://'),
  title: z.string().trim().max(500, 'At most 500 characters').default(''),
  description: z.string().max(5000, 'At most 5000 characters').default(''),
  // Trimmed; duplicates collapse (as the server does).
  tags: z
    .array(tag)
    .max(20, 'At most 20 tags')
    .nullish()
    .transform((t) => [...new Set(t ?? [])]),
})
export type BookmarkForm = z.output<typeof bookmarkSchema>

/** The new-share form of the Sharing page. */
export const grantSchema = z
  .object({
    subject: z.string().min(1, 'Required'),
    resource_id: z.string().default(''),
    relation: z.enum(RELATIONS as [string, ...string[]]),
    expires: z.string().default(''),
  })
  .refine((v) => v.resource_id !== '' || v.subject === 'tenant', { path: ['resource_id'], message: 'Every bookmark can be shared with everyone only' })
  .refine((v) => !v.expires || new Date(v.expires + 'T00:00:00Z').getTime() > Date.now(), { path: ['expires'], message: 'Must be in the future' })

/** "tenant" | "role:<slug>" | "user:<id>" → subject_type/subject_id. */
export function parseSubject(key: string): { subject_type: 'tenant' | 'role' | 'user'; subject_id: string } {
  if (key === 'tenant') return { subject_type: 'tenant', subject_id: '' }
  const i = key.indexOf(':')
  const t = key.slice(0, i)
  return { subject_type: t === 'role' ? 'role' : 'user', subject_id: key.slice(i + 1) }
}
