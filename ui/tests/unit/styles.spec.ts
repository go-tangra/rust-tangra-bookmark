import { describe, expect, it } from 'vitest'
import { ICONS } from '@go-tangra/ui'
import { EXTRA_ICONS } from '../../src/safelist'

const sources = import.meta.glob<string>(['../../src/**/*.{vue,ts}', '!../../src/api/schema.d.ts'], { query: '?raw', import: 'default', eager: true })
const kit = new Set<string>(ICONS)
const safelisted = new Set<string>(EXTRA_ICONS.map((c) => c.replace('icon-[mdi--', 'mdi-').replace(']', '')))
const used = new Set(Object.values(sources).flatMap((src) => [...src.matchAll(/\bmdi-[a-z0-9][a-z0-9-]*/g), ...[...src.matchAll(/icon-\[mdi--([a-z0-9-]+)\]/g)].map((m) => ['mdi-' + m[1]])].map((m) => m[0]!)))

// Tailwind emits only classes it sees; the kit builds icon classes at
// runtime, so every icon is either in the shell's kit set or safelisted in
// this remote's stylesheet. Navigation icons are checked against the kit set
// in remote.spec.ts.
describe('styles', () => {
  it('reads the module sources', () => expect(Object.keys(sources).length).toBeGreaterThan(10))
  it('every icon the pages use is in the kit set or the remote safelist', () => {
    expect([...used].filter((i) => !kit.has(i) && !safelisted.has(i))).toEqual([])
  })
  it('the safelist holds only icons outside the kit set that the pages use', () => {
    expect([...safelisted].filter((i) => kit.has(i))).toEqual([])
    const pages = new Set(Object.entries(sources).filter(([f]) => !f.endsWith('safelist.ts')).flatMap(([, src]) => [...src.matchAll(/\bmdi-[a-z0-9][a-z0-9-]*/g)].map((m) => m[0])))
    expect([...safelisted].filter((i) => !pages.has(i))).toEqual([])
  })
})
