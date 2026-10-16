<script setup lang="ts">
// Who can open, change, remove or share one bookmark: grant to a person, a
// role or everyone in the tenant, with a level and an optional expiry, and
// revoke. Owners grant every level, sharers viewer and sharer only; tenant
// admins hold owner on every bookmark.
import { computed, watch } from 'vue'
import { UiPermissionDrawer, usePermissionGrants } from '@go-tangra/ui'
import { useSharing } from '@/stores/permissions'
import { useDirectory } from '@/stores/directory'
import { grantable, heldRelation, type Relation, type SubjectType } from '@/api/types'

const props = defineProps<{ bookmarkId: string; name: string }>()
const open = defineModel<boolean>({ default: false })
const emit = defineEmits<{ (e: 'changed'): void }>()

const store = useSharing()
const dir = useDirectory()

const access = usePermissionGrants({
  grants: () => store.items,
  effective: () => ({ relation: heldRelation(store.mine) || undefined, canShare: !!store.mine?.permissions.share }),
  grant: async (r) => {
    await store.add({ resource_id: props.bookmarkId, subject_type: r.subject_type as SubjectType, subject_id: r.subject_id ?? '', relation: r.relation as Relation, ...(r.expires_at ? { expires_at: r.expires_at } : {}) })
    emit('changed')
  },
  revoke: async (id) => {
    await store.remove(id)
    emit('changed')
  },
  directory: { roles: () => Object.values(dir.roles), searchUsers: dir.searchUsers, resolveUsers: dir.resolveUsers, userName: dir.userName, roleName: dir.roleName },
  grantable,
})

const hint = computed(() => access.hint.value + (store.mine?.admin ? ' (administrator)' : '') + '. A person you share with directly gets a notice.')
const error = computed(() => access.error.value || store.error)

watch(
  () => [open.value, props.bookmarkId] as const,
  async ([isOpen, id]) => {
    if (!isOpen || !id) return
    await Promise.all([store.load(id), dir.loadRoles()])
    await access.resolve()
  },
  { immediate: true },
)
</script>

<template>
  <UiPermissionDrawer v-model="open" :title="'Share ' + name" :grants="access.grants.value" :subjects="access.subjects.value" :levels="access.levels.value" :can-manage="access.canShare.value" expires :editable-level="false" :hint="hint" :error="error" :busy="access.busy.value || store.loading" data-test="share-drawer" @search="access.search" @grant="access.onGrant" @revoke="access.onRevoke" @change-level="access.onChangeLevel" />
</template>
