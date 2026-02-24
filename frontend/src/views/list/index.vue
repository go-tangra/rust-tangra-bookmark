<script lang="ts" setup>
import type { VxeGridProps } from 'shell/adapter/vxe-table';

import { h } from 'vue';

import { Page, useVbenDrawer, type VbenFormProps } from 'shell/vben/common-ui';
import { LucideEye, LucideTrash, LucidePencil, LucideShield } from 'shell/vben/icons';
import { $t } from 'shell/locales';

import { notification, Space, Button, Tag } from 'ant-design-vue';

import { useVbenVxeGrid } from 'shell/adapter/vxe-table';
import type { Bookmark } from '../../api/services';
import { useBookmarkStore } from '../../stores/bookmark.state';

import BookmarkDrawer from './bookmark-drawer.vue';
import PermissionDrawer from './permission-drawer.vue';

const bookmarkStore = useBookmarkStore();

const formOptions: VbenFormProps = {
  collapsed: false,
  showCollapseButton: false,
  submitOnEnter: true,
  schema: [
    {
      component: 'Input',
      fieldName: 'tagFilter',
      label: $t('bookmark.page.bookmark.tagFilter'),
      componentProps: {
        placeholder: $t('ui.placeholder.input'),
        allowClear: true,
      },
    },
    {
      component: 'Input',
      fieldName: 'query',
      label: $t('ui.table.search'),
      componentProps: {
        placeholder: $t('ui.placeholder.input'),
        allowClear: true,
      },
    },
  ],
};

const gridOptions: VxeGridProps<Bookmark> = {
  height: 'auto',
  stripe: false,
  toolbarConfig: {
    custom: true,
    export: true,
    import: false,
    refresh: true,
    zoom: true,
  },
  exportConfig: {},
  rowConfig: {
    isHover: true,
  },
  pagerConfig: {
    enabled: true,
    pageSize: 20,
    pageSizes: [10, 20, 50, 100],
  },

  proxyConfig: {
    ajax: {
      query: async ({ page }, formValues) => {
        const resp = await bookmarkStore.listBookmarks(
          {
            page: page.currentPage,
            pageSize: page.pageSize,
          },
          {
            tagFilter: formValues?.tagFilter,
          },
        );
        return {
          items: resp.bookmarks ?? [],
          total: resp.total ?? 0,
        };
      },
    },
  },

  columns: [
    { title: $t('ui.table.seq'), type: 'seq', width: 50 },
    {
      title: $t('bookmark.page.bookmark.url'),
      field: 'url',
      minWidth: 200,
      slots: { default: 'url' },
    },
    {
      title: $t('bookmark.page.bookmark.name'),
      field: 'title',
      minWidth: 150,
    },
    {
      title: $t('bookmark.page.bookmark.tags'),
      field: 'tags',
      minWidth: 200,
      slots: { default: 'tags' },
    },
    {
      title: $t('bookmark.page.bookmark.createdAt'),
      field: 'createTime',
      width: 160,
      sortable: true,
    },
    {
      title: $t('ui.table.action'),
      field: 'action',
      fixed: 'right',
      slots: { default: 'action' },
      width: 180,
    },
  ],
};

const [Grid, gridApi] = useVbenVxeGrid({ gridOptions, formOptions });

const [BookmarkDrawerComponent, bookmarkDrawerApi] = useVbenDrawer({
  connectedComponent: BookmarkDrawer,
  onOpenChange(isOpen: boolean) {
    if (!isOpen) {
      gridApi.query();
    }
  },
});

const [PermissionDrawerComponent, permissionDrawerApi] = useVbenDrawer({
  connectedComponent: PermissionDrawer,
  onOpenChange(isOpen: boolean) {
    if (!isOpen) {
      gridApi.query();
    }
  },
});

function openDrawer(row: Bookmark, mode: 'create' | 'edit' | 'view') {
  bookmarkDrawerApi.setData({ row, mode });
  bookmarkDrawerApi.open();
}

function handleView(row: Bookmark) {
  openDrawer(row, 'view');
}

function handleEdit(row: Bookmark) {
  openDrawer(row, 'edit');
}

function handleCreate() {
  openDrawer({} as Bookmark, 'create');
}

function handlePermissions(row: Bookmark) {
  permissionDrawerApi.setData({
    resourceType: 'RESOURCE_TYPE_BOOKMARK',
    resourceId: row.id,
    resourceName: row.title,
  });
  permissionDrawerApi.open();
}

async function handleDelete(row: Bookmark) {
  if (!row.id) return;
  try {
    await bookmarkStore.deleteBookmark(row.id);
    notification.success({
      message: $t('bookmark.page.bookmark.deleteSuccess'),
    });
    await gridApi.query();
  } catch {
    notification.error({ message: $t('ui.notification.delete_failed') });
  }
}
</script>

<template>
  <Page auto-content-height>
    <Grid :table-title="$t('bookmark.page.bookmark.title')">
      <template #toolbar-tools>
        <Button class="mr-2" type="primary" @click="handleCreate">
          {{ $t('bookmark.page.bookmark.create') }}
        </Button>
      </template>
      <template #url="{ row }">
        <a :href="row.url" target="_blank" rel="noopener noreferrer">
          {{ row.url }}
        </a>
      </template>
      <template #tags="{ row }">
        <template v-if="row.tags && row.tags.length">
          <Tag v-for="tag in row.tags" :key="tag" color="blue">
            {{ tag }}
          </Tag>
        </template>
        <span v-else class="text-muted-foreground">-</span>
      </template>
      <template #action="{ row }">
        <Space>
          <Button
            type="link"
            size="small"
            :icon="h(LucideEye)"
            :title="$t('ui.button.view')"
            @click.stop="handleView(row)"
          />
          <Button
            type="link"
            size="small"
            :icon="h(LucidePencil)"
            :title="$t('ui.button.edit')"
            @click.stop="handleEdit(row)"
          />
          <Button
            type="link"
            size="small"
            :icon="h(LucideShield)"
            :title="$t('bookmark.page.permission.title')"
            @click.stop="handlePermissions(row)"
          />
          <a-popconfirm
            :cancel-text="$t('ui.button.cancel')"
            :ok-text="$t('ui.button.ok')"
            :title="$t('bookmark.page.bookmark.confirmDelete')"
            @confirm="handleDelete(row)"
          >
            <Button
              danger
              type="link"
              size="small"
              :icon="h(LucideTrash)"
              :title="$t('ui.button.delete', { moduleName: '' })"
            />
          </a-popconfirm>
        </Space>
      </template>
    </Grid>

    <BookmarkDrawerComponent />
    <PermissionDrawerComponent />
  </Page>
</template>
