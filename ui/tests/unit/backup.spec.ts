import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import { ABILITY_TOKEN } from '@casl/vue'
import { createMongoAbility } from '@casl/ability'
import { useConfirm } from '@go-tangra/ui'
import BackupPage from '@/views/backup/index.vue'
import { backupFileName, readBackup, readText } from '@/stores/backup'
import { answer, body, callsTo, click, fetchMock, mkRouter, refuse } from './helpers'

const backup = { schema_version: 1, module: 'bookmark', exported_at: '2026-10-01T10:00:00Z', bookmarks: [{ id: 'b1', url: 'https://go.dev' }], permissions: [] }
const result = { bookmarks_imported: 3, bookmarks_skipped: 1, permissions_imported: 2, permissions_skipped: 4 }

async function pickFile(w: ReturnType<typeof mount>, content: string, name = 'b.json'): Promise<void> {
  const input = w.find<HTMLInputElement>('input[type=file]').element
  Object.defineProperty(input, 'files', { value: [new File([content], name, { type: 'application/json' })], configurable: true })
  input.dispatchEvent(new Event('change'))
  await flushPromises()
  await new Promise((r) => setTimeout(r, 10)) // FileReader
  await flushPromises()
}

const mountPage = async (rules?: { action: string; subject: string }[]) => {
  const provide = rules ? { [ABILITY_TOKEN as symbol]: createMongoAbility(rules) } : {}
  const w = mount(BackupPage, { global: { plugins: [mkRouter()], provide }, attachTo: document.body })
  await flushPromises()
  return w
}

describe('backup files', () => {
  it('reads only bookmark backups of schema 1', async () => {
    expect(await readBackup(new File(['{'], 'x.json'))).toEqual({ error: 'The file is not JSON.' })
    expect(await readBackup(new File(['{"a":1}'], 'x.json'))).toEqual({ error: 'The file is not a bookmark backup.' })
    expect(await readBackup(new File([JSON.stringify({ ...backup, schema_version: 2 })], 'x.json'))).toEqual({ error: 'Unsupported backup version 2 (expected 1).' })
    expect(await readBackup(new File([JSON.stringify({ ...backup, module: 'paperless' })], 'x.json'))).toEqual({ error: 'The file is a backup of paperless, not of bookmarks.' })
    expect(await readBackup(new File([JSON.stringify(backup)], 'x.json'))).toEqual({ backup })
    expect(backupFileName(new Date('2026-10-16T12:34:56Z'))).toBe('bookmark-backup-2026-10-16-12-34-56.json')
  })
})

describe('backup page', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    document.cookie = '__Host-csrf=tok; Secure; Path=/'
    document.body.innerHTML = ''
  })

  it('exports to a downloaded JSON file', async () => {
    const calls = fetchMock(() => backup)
    const created: Blob[] = []
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: (b: Blob) => (created.push(b), 'blob:x'), revokeObjectURL: () => {} }))
    const clicked = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      expect(this.download).toMatch(/^bookmark-backup-.*\.json$/)
      expect(this.href).toBe('blob:x')
    })
    const w = await mountPage()
    await click(w.find('[data-test=backup-export]').element)
    expect(callsTo(calls, 'POST', '/backup/export').length).toBe(1)
    expect(clicked).toHaveBeenCalledOnce()
    expect(JSON.parse(await readText(created[0]!))).toEqual(backup)
    clicked.mockRestore()
    w.unmount()
  })

  it('imports a picked file in skip mode and shows the counts', async () => {
    const calls = fetchMock(() => result)
    const w = await mountPage()
    expect(w.find('[data-test=backup-import]').attributes('disabled')).toBeDefined()
    await pickFile(w, '{nope')
    expect(w.find('[data-test=backup-error]').text()).toContain('The file is not JSON.')
    await pickFile(w, JSON.stringify(backup))
    expect(w.find('[data-test=backup-summary]').text()).toContain('1 bookmarks and 0 shares')
    await click(w.find('[data-test=backup-import]').element)
    expect(body(calls, 'POST', '/backup/import')).toEqual({ mode: 'skip', backup })
    const tiles = w.find('[data-test=backup-result]').text()
    for (const t of ['Bookmarks imported', '3', 'Bookmarks skipped', 'Shares imported', '2', 'Shares skipped', '4']) expect(tiles).toContain(t)
    w.unmount()
  })

  it('overwrite asks first; a refusal names the entries at fault', async () => {
    const calls = fetchMock(() => refuse(422, { reason: 'validation_failed', detail: { fields: { 'bookmarks[0].url': 'scheme' } } }))
    const w = await mountPage()
    await pickFile(w, JSON.stringify(backup))
    await w.find('select').setValue('overwrite')
    await w.find('[data-test=backup-import]').trigger('click')
    await flushPromises()
    expect(useConfirm().state.pending?.title).toBe('Overwrite existing bookmarks?')
    await answer(false)
    expect(callsTo(calls, 'POST', '/backup/import').length).toBe(0)
    await w.find('[data-test=backup-import]').trigger('click')
    await answer(true)
    expect(body(calls, 'POST', '/backup/import')!.mode).toBe('overwrite')
    expect(w.find('[data-test=backup-error]').text()).toContain('bookmarks[0].url: Must start with http:// or https://')
    expect(w.find('[data-test=backup-result]').exists()).toBe(false)
    w.unmount()
  })

  it('needs the backup permission', async () => {
    fetchMock(() => ({}))
    const w = await mountPage([{ action: 'read', subject: 'Bookmark' }])
    expect(w.text()).toContain('You need the backup permission')
    expect(w.find('[data-test=backup-export]').exists()).toBe(false)
    w.unmount()
  })
})
