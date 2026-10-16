<script setup lang="ts">
// Sharing: the grants of every bookmark the caller may share (tenant admins:
// every grant of the tenant, collection grants included), server-paged and
// filtered by person/role/everyone and by bookmark, with the subjects, the
// granting user and the bookmarks named. New shares from here; an
// administrator can share every bookmark with everyone (no bookmark given).
import { computed, onMounted, ref, watch } from 'vue'
import { UiPage, UiAlert, UiCard, UiButton, UiDataTable, UiDrawer, UiForm, UiSelect, UiCombobox, UiInput, UiBadge, UiHelpLink, useConfirm, useListQuery, useToast, type Column, type SelectOption } from '@go-tangra/ui'
import { useZodForm } from '@go-tangra/ui/forms'
import { ApiError, describe } from '@/api/client'
import { fieldMessage, RELATION_COLORS, RELATIONS, type Bookmark, type Grant } from '@/api/types'
import { GRANT_LIST, useGrants, type GrantFilter } from '@/stores/permissions'
import { useBookmarks } from '@/stores/bookmarks'
import { useDirectory } from '@/stores/directory'
import { grantSchema, parseSubject } from '@/schemas'

const store = useGrants()
const bookmarks = useBookmarks()
const dir = useDirectory()
const confirm = useConfirm()
const toast = useToast()
const lq = useListQuery('grants', GRANT_LIST)

// --- names: subjects and granting users from auth, bookmarks by ID ---
const titles = ref<Record<string, string | null>>({})
async function resolveNames(items: Grant[]): Promise<void> {
  await Promise.all([
    dir.loadRoles(),
    dir.resolveUsers(items.flatMap((g) => [g.subject_type === 'user' ? g.subject_id : undefined, g.granted_by])),
    ...[...new Set(items.map((g) => g.resource_id).filter((id) => id && !(id in titles.value)))].map((id) =>
      bookmarks
        .get(id)
        .then((b) => (titles.value[id] = b.title || b.url))
        .catch(() => (titles.value[id] = null)),
    ),
  ])
}
const subjectName = (g: Pick<Grant, 'subject_type' | 'subject_id'>) => (g.subject_type === 'tenant' ? 'Everyone in the tenant' : g.subject_type === 'role' ? dir.roleName(g.subject_id) : dir.userName(g.subject_id))
const bookmarkName = (g: Grant) => (!g.resource_id ? 'Every bookmark' : titles.value[g.resource_id] ?? g.resource_id)
const expired = (g: Grant) => !!g.expires_at && new Date(g.expires_at).getTime() <= Date.now()

// --- filters: subject (combobox over everyone/roles/users) and bookmark ---
const subjectFilter = ref('')
const bookmarkFilter = ref('')
const userHits = ref<SelectOption[]>([])
const bookmarkHits = ref<SelectOption[]>([])
const subjectOptions = computed<SelectOption[]>(() => [
  { title: 'Everyone in the tenant', value: 'tenant' },
  ...Object.values(dir.roles).map((r) => ({ title: 'Role: ' + r.display_name, value: 'role:' + r.slug })),
  ...userHits.value,
])
async function searchUsers(q: string): Promise<void> {
  userHits.value = (await dir.searchUsers(q)).map((u) => ({ title: u.display_name, value: 'user:' + u.id }))
}
async function searchBookmarks(q: string): Promise<void> {
  try {
    bookmarkHits.value = (await bookmarks.search(q)).map((b: Bookmark) => ({ title: b.title || b.url, value: b.id }))
  } catch {
    bookmarkHits.value = []
  }
}
function filterValue(): GrantFilter {
  const f: GrantFilter = {}
  if (subjectFilter.value) {
    const s = parseSubject(subjectFilter.value)
    f.subject_type = s.subject_type
    if (s.subject_id) f.subject_id = s.subject_id
  }
  if (bookmarkFilter.value) f.resource_id = bookmarkFilter.value
  return f
}

async function load(): Promise<void> {
  const res = await store.list(filterValue(), lq.query.value)
  if (res?.page) lq.clampTo(res.page)
  if (res) await resolveNames(res.items ?? [])
}
watch(lq.query, () => void load())
watch([subjectFilter, bookmarkFilter], () => (lq.page.value !== 1 ? lq.resetPage() : void load()))
onMounted(async () => {
  void load()
  void searchBookmarks('')
  try {
    admin.value = (await bookmarks.effective()).admin
  } catch {
    admin.value = false
  }
})

const columns: Column<Grant>[] = [
  { key: 'resource_id', label: 'Bookmark', format: bookmarkName },
  { key: 'subject_type', label: 'Kind', sortable: true, hideOnStack: true },
  { key: 'subject_id', label: 'Shared with', format: subjectName },
  { key: 'relation', label: 'Level', sortable: true },
  { key: 'expires_at', label: 'Expires', format: (g) => (g.expires_at ? new Date(g.expires_at).toLocaleString() : 'Never'), hideOnStack: true },
  { key: 'granted_by', label: 'Shared by', format: (g) => (g.granted_by ? dir.userName(g.granted_by) : '—'), hideOnStack: true },
  { key: 'granted_at', label: 'Shared', format: (g) => new Date(g.granted_at).toLocaleString(), sortable: true, defaultDir: 'desc', hideOnStack: true },
]

async function revoke(g: Grant): Promise<void> {
  if (!(await confirm.ask({ title: 'Remove this share?', text: subjectName(g) + ' loses ' + g.relation + ' access to ' + bookmarkName(g) + '.', confirmLabel: 'Remove', danger: true }))) return
  try {
    await store.remove(g)
    toast.success('Share removed')
    void load()
  } catch (e) {
    toast.error('Not removed', describe(e))
  }
}

// --- new share ---
const admin = ref(false)
const creating = ref(false)
const levelOptions: SelectOption[] = RELATIONS.map((r) => ({ title: r, value: r }))
const form = useZodForm(grantSchema, {
  onSubmit: async (v) => {
    const s = parseSubject(v.subject)
    try {
      return await store.grant({ ...s, ...(v.resource_id ? { resource_id: v.resource_id } : {}), relation: v.relation as Grant['relation'], ...(v.expires ? { expires_at: new Date(v.expires + 'T00:00:00Z').toISOString() } : {}) })
    } catch (e) {
      const d = e instanceof ApiError ? (e.detail as { fields?: Record<string, unknown> } | undefined) : undefined
      if (d?.fields) d.fields = Object.fromEntries(Object.entries(d.fields).map(([k, c]) => [k === 'subject_id' || k === 'subject_type' ? 'subject' : k === 'expires_at' ? 'expires' : k, fieldMessage(c)]))
      if (e instanceof ApiError && e.reason === 'conflict') form.setFieldError('subject', 'Already holds this level on the bookmark')
      throw e
    }
  },
  onSuccess: () => {
    creating.value = false
    toast.success('Shared')
    void load()
  },
})
function startNew(): void {
  form.reset({ subject: '', resource_id: '', relation: 'viewer', expires: '' })
  void dir.loadRoles()
  creating.value = true
}
</script>

<template>
  <UiPage title="Sharing">
    <template #actions>
      <UiHelpLink module="bookmark" slug="guides/sharing" icon-only />
      <UiButton variant="text" icon="mdi-refresh" icon-only label="Refresh" @click="load" />
      <UiButton icon="mdi-account-plus-outline" data-test="grant-new" @click="startNew">New share</UiButton>
    </template>
    <template #filters>
      <div class="grid w-full grid-cols-1 gap-2 md:max-w-2xl md:grid-cols-2">
        <UiCombobox id="grant-filter-subject" v-model="subjectFilter" label="Shared with" placeholder="Anyone" :options="subjectOptions" @search="searchUsers" />
        <UiCombobox id="grant-filter-bookmark" v-model="bookmarkFilter" label="Bookmark" placeholder="Any bookmark" :options="bookmarkHits" @search="searchBookmarks" />
      </div>
    </template>
    <UiAlert v-if="store.error" kind="error" class="mb-3">{{ store.error }}</UiAlert>
    <UiCard :padded="false">
      <UiDataTable :items="store.items" :columns="columns" :loading="store.loading" :total="store.total" :page="lq.page.value" :page-size="lq.pageSize.value" :sort="lq.sort.value" caption="Shares" empty-title="No shares" :row-attrs="(g) => ({ 'data-test': 'grant-row-' + g.id })" data-test="grants-table" @update:page="lq.setPage" @update:page-size="lq.setPageSize" @update:sort="lq.setSort">
        <template #cell-relation="{ row }">
          <UiBadge :color="RELATION_COLORS[row.relation]" size="sm" soft>{{ row.relation }}</UiBadge>
          <UiBadge v-if="expired(row)" color="neutral" size="sm" class="ms-1">expired</UiBadge>
        </template>
        <template #actions="{ row }">
          <UiButton variant="text" color="error" size="xs" icon="mdi-delete-outline" icon-only :label="'Remove share for ' + subjectName(row)" :data-test="'grant-remove-' + row.id" @click="revoke(row)" />
        </template>
      </UiDataTable>
    </UiCard>

    <UiDrawer v-model="creating" title="New share" size="md" data-test="grant-form">
      <UiForm :form="form">
        <div class="flex flex-col gap-3">
          <UiCombobox v-bind="form.field('subject')" label="Share with" :options="subjectOptions" required @search="searchUsers" />
          <UiCombobox v-bind="form.field('resource_id')" :label="admin ? 'Bookmark (empty: every bookmark)' : 'Bookmark'" :options="bookmarkHits" :required="!admin" @search="searchBookmarks" />
          <UiSelect v-bind="form.field('relation')" label="Level" :options="levelOptions" :clearable="false" required />
          <UiInput v-bind="form.field('expires')" label="Expires (optional)" type="date" />
          <p class="text-xs text-base-content/70">Owners give every level, sharers viewer and sharer only.<template v-if="admin"> Leave the bookmark empty and choose everyone to give a level on every bookmark.</template></p>
        </div>
      </UiForm>
      <template #actions>
        <UiButton variant="text" @click="creating = false">Cancel</UiButton>
        <UiButton :loading="form.submitting.value" data-test="grant-save" @click="form.submit()">Share</UiButton>
      </template>
    </UiDrawer>
  </UiPage>
</template>
