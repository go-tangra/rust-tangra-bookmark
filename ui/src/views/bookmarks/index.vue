<script setup lang="ts">
// The bookmarks the caller can open: a server-paged table (sort on the
// server's fields; page / size / sort in the URL), a title/address search and
// a tag filter (every tag must be present). A row opens the bookmark; per-row
// actions follow the row's own permissions and the caller's abilities. A
// bookmark.shared event reloads the page.
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { UiPage, UiAlert, UiCard, UiButton, UiDataTable, UiDrawer, UiForm, UiInput, UiTextarea, UiKeyValueTable, UiBadge, UiHelpLink, useConfirm, useListQuery, useToast, type Column } from '@go-tangra/ui'
import { useZodForm } from '@go-tangra/ui/forms'
import { ApiError, describe } from '@/api/client'
import { fieldMessage, heldRelation, RELATION_COLORS, type Bookmark, type Effective } from '@/api/types'
import { BOOKMARK_LIST, useBookmarks } from '@/stores/bookmarks'
import { useDirectory } from '@/stores/directory'
import { EVENT_SHARED, onEvent } from '@/stores/live'
import { bookmarkSchema } from '@/schemas'
import { useCan } from '@/ability'
import TagListInput from '@/components/TagListInput.vue'
import SharingDrawer from '@/components/SharingDrawer.vue'

const store = useBookmarks()
const dir = useDirectory()
const confirm = useConfirm()
const toast = useToast()
const can = useCan()
const lq = useListQuery('bookmarks', BOOKMARK_LIST)

// --- filters: search text (debounced) and tags ---
const search = ref('')
const tags = ref<string[]>([])
async function load(): Promise<void> {
  const res = await store.list({ q: search.value.trim() || undefined, tag: tags.value.length ? [...tags.value] : undefined }, lq.query.value)
  if (res?.page) lq.clampTo(res.page) // a page beyond the end answers the last page
}
const filtersChanged = () => (lq.page.value !== 1 ? lq.resetPage() : void load())
watch(lq.query, () => void load())
let timer: ReturnType<typeof setTimeout> | undefined
watch(search, () => {
  clearTimeout(timer)
  timer = setTimeout(filtersChanged, 300)
})
watch(tags, filtersChanged, { deep: true })
function addTag(t: string): void {
  if (!tags.value.includes(t) && tags.value.length < 10) tags.value = [...tags.value, t]
}

let off: (() => void) | null = null
onMounted(() => {
  void load()
  off = onEvent(EVENT_SHARED, () => void load())
})
onUnmounted(() => {
  off?.()
  clearTimeout(timer)
})

const when = (s: string) => new Date(s).toLocaleString()
const safeHref = (u: string) => (/^https?:\/\//i.test(u) ? u : undefined)
const columns: Column<Bookmark>[] = [
  { key: 'title', label: 'Title', sortable: true },
  { key: 'url', label: 'Address', sortable: true },
  { key: 'tags', label: 'Tags', hideOnStack: true },
  { key: 'updated_at', label: 'Changed', format: (b) => when(b.updated_at), sortable: true, defaultDir: 'desc', hideOnStack: true },
  { key: 'created_at', label: 'Created', format: (b) => when(b.created_at), sortable: true, defaultDir: 'desc', hideOnStack: true },
]

// --- what the caller may do with a row: the row's permissions and the ability ---
const canCreate = computed(() => can('create', 'Bookmark'))
const canEdit = (b: Bookmark) => b.permissions.write && can('update', 'Bookmark')
const canRemove = (b: Bookmark) => b.permissions.delete && can('delete', 'Bookmark')
const canShare = (b: Bookmark) => b.permissions.share && can('manage', 'BookmarkPermission')

// --- create / edit drawer ---
const editing = ref(false)
const editTarget = ref<Bookmark | null>(null)
const form = useZodForm(bookmarkSchema, {
  onSubmit: async (v) => {
    try {
      return editTarget.value ? await store.update(editTarget.value.id, v) : await store.create(v)
    } catch (e) {
      // The server names fields with codes (scheme, too_long, …): put them in words.
      const d = e instanceof ApiError ? (e.detail as { fields?: Record<string, unknown> } | undefined) : undefined
      if (d?.fields) d.fields = Object.fromEntries(Object.entries(d.fields).map(([k, c]) => [k.replace(/\.\d+$|\[\d+\]$/, ''), fieldMessage(c)]))
      throw e
    }
  },
  onSuccess: (b) => {
    editing.value = false
    toast.success(editTarget.value ? 'Bookmark saved' : 'Bookmark created')
    if (selected.value && b && selected.value.id === b.id) selected.value = b
    void load()
  },
})
function startNew(): void {
  editTarget.value = null
  form.reset({ url: '', title: '', description: '', tags: [] })
  editing.value = true
}
function startEdit(b: Bookmark): void {
  editTarget.value = b
  form.reset({ url: b.url, title: b.title, description: b.description, tags: [...b.tags] })
  viewing.value = false
  editing.value = true
}

// --- detail drawer: the bookmark and the caller's own access to it ---
const viewing = ref(false)
const selected = ref<Bookmark | null>(null)
const mine = ref<Effective | null>(null)
const error = ref('')
async function view(b: Bookmark): Promise<void> {
  selected.value = b
  mine.value = null
  error.value = ''
  viewing.value = true
  void dir.resolveUsers([b.created_by])
  try {
    const eff = await store.effective(b.id)
    if (selected.value?.id !== b.id) return
    mine.value = eff
    await Promise.all([dir.resolveUsers(eff.grants.map((g) => (g.subject_type === 'user' ? g.subject_id : undefined))), dir.loadRoles()])
  } catch (e) {
    if (selected.value?.id === b.id) error.value = describe(e)
  }
}
const meta = computed(() => {
  const b = selected.value
  if (!b) return []
  return [
    { label: 'Address', value: b.url, copyable: true },
    { label: 'Description', value: b.description || '—' },
    { label: 'Created by', value: b.created_by ? dir.userName(b.created_by) : '—' },
    { label: 'Created', value: when(b.created_at) },
    { label: 'Changed', value: when(b.updated_at) },
  ]
})
const allowed = computed(() => {
  const p = selected.value?.permissions
  if (!p) return []
  return (['read', 'write', 'delete', 'share'] as const).filter((k) => p[k]).map((k) => ({ read: 'open', write: 'change', delete: 'remove', share: 'share' })[k])
})
const via = computed(() =>
  (mine.value?.grants ?? []).map((g) => ({
    id: g.id,
    relation: g.relation,
    who: g.subject_type === 'tenant' ? 'Everyone in the tenant' : g.subject_type === 'role' ? 'Role: ' + dir.roleName(g.subject_id) : dir.userName(g.subject_id),
    scope: g.resource_id ? '' : ' (every bookmark)',
    expires: g.expires_at ? ' until ' + new Date(g.expires_at).toLocaleDateString() : '',
  })),
)

// --- share and remove ---
const sharing = ref(false)
const shareTarget = ref<Bookmark | null>(null)
function share(b: Bookmark): void {
  shareTarget.value = b
  viewing.value = false // one drawer at a time
  sharing.value = true
}
async function remove(b: Bookmark): Promise<void> {
  if (!(await confirm.ask({ title: 'Remove this bookmark?', text: (b.title || b.url) + ' and every share of it are deleted. This cannot be undone.', confirmLabel: 'Remove', danger: true }))) return
  try {
    await store.remove(b.id)
    viewing.value = false
    toast.success('Bookmark removed')
    void load()
  } catch (e) {
    toast.error('Not removed', describe(e))
  }
}
const label = (b: Bookmark) => b.title || b.url
</script>

<template>
  <UiPage title="Bookmarks">
    <template #actions>
      <UiHelpLink module="bookmark" slug="getting-started" icon-only />
      <UiButton variant="text" icon="mdi-refresh" icon-only label="Refresh" @click="load" />
      <UiButton v-if="canCreate" icon="mdi-bookmark-plus-outline" data-test="bookmark-new" @click="startNew">New bookmark</UiButton>
    </template>
    <template #filters>
      <div class="grid w-full grid-cols-1 gap-2 md:max-w-2xl md:grid-cols-2">
        <UiInput id="bookmark-search" v-model="search" label="Search" type="search" placeholder="Part of a title or address" size="sm" />
        <TagListInput id="bookmark-tags" v-model="tags" label="Tags" hint="Enter to add; bookmarks carrying every tag" :max="10" />
      </div>
    </template>
    <UiAlert v-if="store.error" kind="error" class="mb-3">{{ store.error }}</UiAlert>
    <UiCard :padded="false">
      <UiDataTable :items="store.items" :columns="columns" :loading="store.loading" :total="store.total" :page="lq.page.value" :page-size="lq.pageSize.value" :sort="lq.sort.value" caption="Bookmarks" :empty-title="search || tags.length ? 'No bookmark matches' : 'No bookmarks yet'" clickable :row-attrs="(b) => ({ 'data-test': 'bookmark-row-' + b.id })" data-test="bookmarks-table" @row-click="view" @update:page="lq.setPage" @update:page-size="lq.setPageSize" @update:sort="lq.setSort">
        <template #cell-title="{ row }">
          <span class="font-medium">{{ row.title || '—' }}</span>
        </template>
        <template #cell-url="{ row }">
          <a :href="safeHref(row.url)" target="_blank" rel="noopener noreferrer" class="link link-primary break-all" @click.stop>{{ row.url }}</a>
        </template>
        <template #cell-tags="{ row }">
          <span class="flex flex-wrap gap-1">
            <button v-for="t in row.tags" :key="t" type="button" class="badge badge-soft badge-primary badge-sm" :title="'Show bookmarks tagged ' + t" @click.stop="addTag(t)">{{ t }}</button>
          </span>
        </template>
        <template #actions="{ row }">
          <span class="flex justify-end gap-0.5">
            <a :href="safeHref(row.url)" target="_blank" rel="noopener noreferrer" class="btn btn-text btn-xs btn-circle" :aria-label="'Open ' + label(row)" :data-test="'bookmark-open-' + row.id"><span class="icon-[mdi--open-in-new] size-4" aria-hidden="true" /></a>
            <UiButton v-if="canEdit(row)" variant="text" size="xs" icon="mdi-pencil-outline" icon-only :label="'Edit ' + label(row)" :data-test="'bookmark-edit-' + row.id" @click="startEdit(row)" />
            <UiButton v-if="canShare(row)" variant="text" size="xs" icon="mdi-shield-account-outline" icon-only :label="'Share ' + label(row)" :data-test="'bookmark-share-' + row.id" @click="share(row)" />
            <UiButton v-if="canRemove(row)" variant="text" color="error" size="xs" icon="mdi-delete-outline" icon-only :label="'Remove ' + label(row)" :data-test="'bookmark-remove-' + row.id" @click="remove(row)" />
          </span>
        </template>
      </UiDataTable>
    </UiCard>

    <UiDrawer v-model="editing" :title="editTarget ? 'Edit bookmark' : 'New bookmark'" size="md" data-test="bookmark-form">
      <UiForm :form="form">
        <div class="flex flex-col gap-3">
          <UiInput v-bind="form.field('url')" label="Address" type="url" placeholder="https://" required />
          <UiInput v-bind="form.field('title')" label="Title (optional)" />
          <UiTextarea v-bind="form.field('description')" label="Description (optional)" :rows="4" />
          <TagListInput v-bind="form.field('tags')" label="Tags (optional)" :max="20" />
        </div>
      </UiForm>
      <template #actions>
        <UiButton variant="text" @click="editing = false">Cancel</UiButton>
        <UiButton :loading="form.submitting.value" data-test="bookmark-save" @click="form.submit()">{{ editTarget ? 'Save' : 'Create' }}</UiButton>
      </template>
    </UiDrawer>

    <UiDrawer v-model="viewing" :title="selected ? label(selected) : 'Bookmark'" size="lg" data-test="bookmark-view">
      <template v-if="selected">
        <UiAlert v-if="error" kind="error" class="mb-3">{{ error }}</UiAlert>
        <div v-if="selected.tags.length" class="mb-3 flex flex-wrap gap-1">
          <UiBadge v-for="t in selected.tags" :key="t" color="primary" soft>{{ t }}</UiBadge>
        </div>
        <UiKeyValueTable :items="meta" />
        <section class="mt-4" aria-labelledby="bookmark-access">
          <h3 id="bookmark-access" class="mb-1 text-sm font-semibold">Your access</h3>
          <p class="text-sm" data-test="bookmark-allowed">
            You may {{ allowed.join(', ') }}<span v-if="mine">: <UiBadge :color="RELATION_COLORS[heldRelation(mine) || 'viewer']" size="sm" soft>{{ heldRelation(mine) }}</UiBadge></span><span v-if="mine?.admin"> as an administrator</span>.
          </p>
          <ul v-if="via.length" class="mt-1 list-disc ps-5 text-sm text-base-content/80" data-test="bookmark-via">
            <li v-for="g in via" :key="g.id">{{ g.relation }} for {{ g.who }}{{ g.scope }}{{ g.expires }}</li>
          </ul>
        </section>
      </template>
      <template #actions>
        <template v-if="selected">
          <a :href="safeHref(selected.url)" target="_blank" rel="noopener noreferrer" class="btn btn-primary btn-soft btn-sm"><span class="icon-[mdi--open-in-new] size-4" aria-hidden="true" />Open</a>
          <UiButton v-if="canEdit(selected)" size="sm" variant="soft" icon="mdi-pencil-outline" data-test="view-edit" @click="startEdit(selected)">Edit</UiButton>
          <UiButton v-if="canShare(selected)" size="sm" variant="soft" icon="mdi-shield-account-outline" data-test="view-share" @click="share(selected)">Share</UiButton>
          <span class="grow" />
          <UiButton v-if="canRemove(selected)" size="sm" variant="soft" color="error" icon="mdi-delete-outline" data-test="view-remove" @click="remove(selected)">Remove</UiButton>
        </template>
      </template>
    </UiDrawer>

    <SharingDrawer v-if="shareTarget" v-model="sharing" :bookmark-id="shareTarget.id" :name="label(shareTarget)" />
  </UiPage>
</template>
