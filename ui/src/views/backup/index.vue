<script setup lang="ts">
// Bookmark backup: export every bookmark and share of the tenant to a JSON
// file (schema 1) and import one back, skipping or overwriting what exists.
// The import is all or nothing; the result names how much was imported and
// skipped, a refusal the entries at fault.
import { computed, ref } from 'vue'
import { UiPage, UiAlert, UiCard, UiButton, UiFilePicker, UiSelect, UiStatTile, UiStatGrid, UiHelpLink, useConfirm, useToast, type SelectOption } from '@go-tangra/ui'
import { ApiError, describe } from '@/api/client'
import { fieldMessage, type Backup, type ImportMode, type ImportResult } from '@/api/types'
import { download, exportBackup, importBackup, readBackup, MAX_IMPORT_BYTES } from '@/stores/backup'
import { useCan } from '@/ability'

const can = useCan()
const allowed = computed(() => can('manage', 'BookmarkBackup'))
const confirm = useConfirm()
const toast = useToast()

const exporting = ref(false)
const exportError = ref('')
async function doExport(): Promise<void> {
  exporting.value = true
  exportError.value = ''
  try {
    const b = await exportBackup()
    download(b)
    toast.success('Backup exported', b.bookmarks.length + ' bookmarks, ' + b.permissions.length + ' shares')
  } catch (e) {
    exportError.value = describe(e)
  } finally {
    exporting.value = false
  }
}

const file = ref<File | null>(null)
const mode = ref<ImportMode>('skip')
const modes: SelectOption[] = [
  { title: 'Skip: keep what exists, add what is missing', value: 'skip' },
  { title: 'Overwrite: replace what exists with the file', value: 'overwrite' },
]
const parsed = ref<Backup | null>(null)
const importError = ref('')
const problems = ref<string[]>([])
const result = ref<ImportResult | null>(null)
const importing = ref(false)

async function pick(f: unknown): Promise<void> {
  file.value = f instanceof File ? f : null
  parsed.value = null
  importError.value = ''
  problems.value = []
  result.value = null
  if (!file.value) return
  const r = await readBackup(file.value)
  if ('error' in r) importError.value = r.error
  else parsed.value = r.backup
}

async function doImport(): Promise<void> {
  if (!parsed.value) return
  if (mode.value === 'overwrite' && !(await confirm.ask({ title: 'Overwrite existing bookmarks?', text: 'Bookmarks and shares in the file replace those that exist with the same ID or level.', confirmLabel: 'Overwrite', danger: true }))) return
  importing.value = true
  importError.value = ''
  problems.value = []
  result.value = null
  try {
    result.value = await importBackup(parsed.value, mode.value)
    toast.success('Backup imported')
  } catch (e) {
    importError.value = describe(e)
    const d = e instanceof ApiError ? (e.detail as { fields?: Record<string, unknown> } | undefined) : undefined
    if (d?.fields) problems.value = Object.entries(d.fields).map(([k, c]) => k + ': ' + fieldMessage(c))
  } finally {
    importing.value = false
  }
}
</script>

<template>
  <UiPage title="Bookmark backup">
    <template #actions><UiHelpLink module="bookmark" slug="guides/backup" icon-only /></template>
    <UiAlert v-if="!allowed" kind="info">You need the backup permission to export or import bookmarks.</UiAlert>
    <div v-else class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <UiCard title="Export" subtitle="Every bookmark and share of your organisation, with their IDs">
        <UiAlert v-if="exportError" kind="error" class="mb-3">{{ exportError }}</UiAlert>
        <p class="mb-3 text-sm text-base-content/80">The file holds every address and who may see it: keep it confidential.</p>
        <UiButton icon="mdi-download" :loading="exporting" data-test="backup-export" @click="doExport">Export</UiButton>
      </UiCard>
      <UiCard title="Import" subtitle="All or nothing; another organisation's backup writes into yours only">
        <div class="flex flex-col gap-3">
          <UiFilePicker id="backup-file" :model-value="file" label="Backup file" accept="application/json,.json" :max-bytes="MAX_IMPORT_BYTES" data-test="backup-file" @update:model-value="pick" @too-large="importError = 'The file is larger than 16 MB.'" />
          <p v-if="parsed" class="text-sm" data-test="backup-summary">{{ parsed.bookmarks.length }} bookmarks and {{ parsed.permissions.length }} shares<span v-if="parsed.exported_at">, exported {{ new Date(parsed.exported_at).toLocaleString() }}</span>.</p>
          <UiSelect id="backup-mode" v-model="mode" label="Mode" :options="modes" :clearable="false" required />
          <UiAlert v-if="importError" kind="error" data-test="backup-error">
            {{ importError }}
            <ul v-if="problems.length" class="mt-1 list-disc ps-5">
              <li v-for="p in problems" :key="p">{{ p }}</li>
            </ul>
          </UiAlert>
          <div><UiButton icon="mdi-upload" :disabled="!parsed" :loading="importing" data-test="backup-import" @click="doImport">Import</UiButton></div>
        </div>
      </UiCard>
      <UiCard v-if="result" title="Imported" class="lg:col-span-2" data-test="backup-result">
        <UiStatGrid>
          <UiStatTile title="Bookmarks imported" :value="result.bookmarks_imported" color="success" />
          <UiStatTile title="Bookmarks skipped" :value="result.bookmarks_skipped" color="neutral" />
          <UiStatTile title="Shares imported" :value="result.permissions_imported" color="success" />
          <UiStatTile title="Shares skipped" :value="result.permissions_skipped" color="neutral" />
        </UiStatGrid>
      </UiCard>
    </div>
  </UiPage>
</template>
