<script lang="ts" setup>
import { ref, h } from 'vue';

import { Page } from 'shell/vben/common-ui';
import { LucideTrash } from 'shell/vben/icons';
import { $t } from 'shell/locales';

import {
  Table,
  Button,
  notification,
  Modal,
  Tag,
  Spin,
} from 'ant-design-vue';
import type { ColumnsType } from 'ant-design-vue/es/table';

import { useBookmarkPermissionStore } from '../../stores/permission.state';

const permissionStore = useBookmarkPermissionStore();

const loading = ref(false);
const permissions = ref<any[]>([]);

function subjectTypeToName(type: string | undefined) {
  switch (type) {
    case 'SUBJECT_TYPE_USER':
      return $t('bookmark.page.permission.user');
    case 'SUBJECT_TYPE_ROLE':
      return $t('bookmark.page.permission.role');
    case 'SUBJECT_TYPE_TENANT':
      return $t('bookmark.page.permission.tenant');
    default:
      return type ?? '';
  }
}

function relationToName(relation: string | undefined) {
  switch (relation) {
    case 'RELATION_OWNER':
      return $t('bookmark.page.permission.owner');
    case 'RELATION_EDITOR':
      return $t('bookmark.page.permission.editor');
    case 'RELATION_VIEWER':
      return $t('bookmark.page.permission.viewer');
    case 'RELATION_SHARER':
      return $t('bookmark.page.permission.sharer');
    default:
      return relation ?? '';
  }
}

function relationToColor(relation: string | undefined) {
  switch (relation) {
    case 'RELATION_OWNER':
      return 'red';
    case 'RELATION_EDITOR':
      return 'orange';
    case 'RELATION_VIEWER':
      return 'blue';
    case 'RELATION_SHARER':
      return 'purple';
    default:
      return 'default';
  }
}

function formatDateTime(value: string | undefined) {
  if (!value) return $t('bookmark.page.permission.noExpiry');
  try {
    return new Date(value).toLocaleString();
  } catch {
    return value;
  }
}

async function loadPermissions() {
  loading.value = true;
  try {
    const resp = await permissionStore.listPermissions(
      'RESOURCE_TYPE_BOOKMARK',
      '',
    );
    permissions.value = resp.permissions ?? [];
  } catch (e) {
    console.error('Failed to load permissions:', e);
    notification.error({ message: $t('ui.notification.load_failed') });
  } finally {
    loading.value = false;
  }
}

async function handleRevoke(permission: any) {
  Modal.confirm({
    title: $t('bookmark.page.permission.revokeAccess'),
    content: $t('bookmark.page.permission.revokeConfirm'),
    okText: $t('ui.button.ok'),
    cancelText: $t('ui.button.cancel'),
    onOk: async () => {
      try {
        await permissionStore.revokeAccess(
          permission.resourceType,
          permission.resourceId,
          permission.subjectType,
          permission.subjectId ?? '',
          permission.relation,
        );
        notification.success({
          message: $t('bookmark.page.permission.revokeSuccess'),
        });
        await loadPermissions();
      } catch (e) {
        console.error('Failed to revoke access:', e);
        notification.error({ message: $t('ui.notification.operation_failed') });
      }
    },
  });
}

const columns: ColumnsType<any> = [
  {
    title: $t('bookmark.page.permission.resourceId'),
    dataIndex: 'resourceId',
    key: 'resourceId',
    width: 200,
    ellipsis: true,
  },
  {
    title: $t('bookmark.page.permission.subjectType'),
    dataIndex: 'subjectType',
    key: 'subjectType',
    width: 100,
    customRender: ({ text }) => subjectTypeToName(text),
  },
  {
    title: $t('bookmark.page.permission.subject'),
    dataIndex: 'subjectId',
    key: 'subjectId',
    width: 200,
    ellipsis: true,
  },
  {
    title: $t('bookmark.page.permission.relation'),
    dataIndex: 'relation',
    key: 'relation',
    width: 120,
    customRender: ({ text }) =>
      h(Tag, { color: relationToColor(text) }, () => relationToName(text)),
  },
  {
    title: $t('bookmark.page.permission.expiresAt'),
    dataIndex: 'expiresAt',
    key: 'expiresAt',
    width: 180,
    customRender: ({ text }) => formatDateTime(text),
  },
  {
    title: $t('ui.table.action'),
    key: 'action',
    width: 80,
    fixed: 'right',
  },
];

// Load on mount
loadPermissions();
</script>

<template>
  <Page auto-content-height>
    <Spin :spinning="loading">
      <Table
        :columns="columns"
        :data-source="permissions"
        :pagination="{ pageSize: 20, showSizeChanger: true }"
        :scroll="{ x: 900 }"
        row-key="id"
        size="small"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'action'">
            <Button
              danger
              type="text"
              size="small"
              :icon="h(LucideTrash)"
              :title="$t('bookmark.page.permission.revokeAccess')"
              @click="handleRevoke(record)"
            />
          </template>
        </template>

        <template #emptyText>
          <div class="py-8 text-center">
            <span class="text-muted-foreground">{{
              $t('ui.text.no_data')
            }}</span>
          </div>
        </template>
      </Table>
    </Spin>
  </Page>
</template>
