import { api } from '@/api/client'
import type { Backup, ImportMode, ImportResult } from '@/api/types'

/** The gateway's limit on an import body (x-freya-max-body-bytes). */
export const MAX_IMPORT_BYTES = 16 * 1024 * 1024

/** Every bookmark and grant of the caller's tenant (schema 1). */
export const exportBackup = () => api<Backup>('POST', 'backup/export')

/** Restores a backup into the caller's tenant (all or nothing). */
export const importBackup = (backup: Backup, mode: ImportMode) => api<ImportResult>('POST', 'backup/import', { mode, backup })

/** The download name of an export taken at `at`. */
export function backupFileName(at: Date = new Date()): string {
  return 'bookmark-backup-' + at.toISOString().slice(0, 19).replace(/[:T]/g, '-') + '.json'
}

/** Hands the export to the browser as a file download. */
export function download(backup: Backup, name = backupFileName()): void {
  const blob = new Blob([JSON.stringify(backup, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.rel = 'noopener'
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 0)
}

/** A file's text (FileReader: Blob.text is missing in older engines). */
export function readText(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve(String(r.result ?? ''))
    r.onerror = () => reject(r.error ?? new Error('unreadable'))
    r.readAsText(file)
  })
}

/** The backup in a picked file, or an explanation of why it is not one. */
export async function readBackup(file: File): Promise<{ backup: Backup } | { error: string }> {
  let data: unknown
  try {
    data = JSON.parse(await readText(file))
  } catch {
    return { error: 'The file is not JSON.' }
  }
  const b = data as Partial<Backup> | null
  if (!b || typeof b !== 'object' || !Array.isArray(b.bookmarks) || !Array.isArray(b.permissions)) return { error: 'The file is not a bookmark backup.' }
  if (b.schema_version !== 1) return { error: 'Unsupported backup version ' + String(b.schema_version) + ' (expected 1).' }
  if (b.module !== undefined && b.module !== 'bookmark') return { error: 'The file is a backup of ' + String(b.module) + ', not of bookmarks.' }
  return { backup: b as Backup }
}
