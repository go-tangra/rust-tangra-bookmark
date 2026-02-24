<script lang="ts" setup>
import { ref, computed } from 'vue';

import { useVbenDrawer } from 'shell/vben/common-ui';
import { $t } from 'shell/locales';

import {
  Form,
  FormItem,
  Input,
  Textarea,
  Select,
  Button,
  notification,
  Descriptions,
  DescriptionsItem,
  Tag,
  Space,
} from 'ant-design-vue';

import type { Bookmark } from '../../api/services';
import { useBookmarkStore } from '../../stores/bookmark.state';

const bookmarkStore = useBookmarkStore();

const data = ref<{
  mode: 'create' | 'edit' | 'view';
  row?: Bookmark;
}>();
const loading = ref(false);

const formState = ref<{
  url: string;
  title: string;
  description: string;
  tags: string[];
}>({
  url: '',
  title: '',
  description: '',
  tags: [],
});

const title = computed(() => {
  switch (data.value?.mode) {
    case 'create':
      return $t('bookmark.page.bookmark.create');
    case 'edit':
      return $t('bookmark.page.bookmark.edit');
    default:
      return $t('bookmark.page.bookmark.view');
  }
});

const isCreateMode = computed(() => data.value?.mode === 'create');
const isEditMode = computed(() => data.value?.mode === 'edit');
const isViewMode = computed(() => data.value?.mode === 'view');

async function handleSubmit() {
  loading.value = true;
  try {
    if (isCreateMode.value) {
      await bookmarkStore.createBookmark({
        url: formState.value.url,
        title: formState.value.title,
        description: formState.value.description,
        tags: formState.value.tags,
      });
      notification.success({
        message: $t('bookmark.page.bookmark.createSuccess'),
      });
    } else if (isEditMode.value && data.value?.row?.id) {
      await bookmarkStore.updateBookmark(data.value.row.id, {
        url: formState.value.url,
        title: formState.value.title,
        description: formState.value.description,
        tags: formState.value.tags,
        updateTags: true,
      });
      notification.success({
        message: $t('bookmark.page.bookmark.updateSuccess'),
      });
    }
    drawerApi.close();
  } catch (e) {
    console.error('Failed to save bookmark:', e);
    notification.error({
      message: isCreateMode.value
        ? $t('ui.notification.create_failed')
        : $t('ui.notification.update_failed'),
    });
  } finally {
    loading.value = false;
  }
}

function resetForm() {
  formState.value = {
    url: '',
    title: '',
    description: '',
    tags: [],
  };
}

const [Drawer, drawerApi] = useVbenDrawer({
  onCancel() {
    drawerApi.close();
  },

  async onOpenChange(isOpen) {
    if (isOpen) {
      data.value = drawerApi.getData() as {
        mode: 'create' | 'edit' | 'view';
        row?: Bookmark;
      };

      if (data.value?.mode === 'create') {
        resetForm();
      } else if (data.value?.row) {
        formState.value = {
          url: data.value.row.url ?? '',
          title: data.value.row.title ?? '',
          description: data.value.row.description ?? '',
          tags: data.value.row.tags ?? [],
        };
      }
    }
  },
});

const bookmark = computed(() => data.value?.row);
</script>

<template>
  <Drawer :title="title" :footer="false">
    <!-- View Mode -->
    <template v-if="bookmark && isViewMode">
      <Descriptions :column="1" bordered size="small">
        <DescriptionsItem :label="$t('bookmark.page.bookmark.url')">
          <a
            :href="bookmark.url"
            target="_blank"
            rel="noopener noreferrer"
          >
            {{ bookmark.url || '-' }}
          </a>
        </DescriptionsItem>
        <DescriptionsItem :label="$t('bookmark.page.bookmark.name')">
          {{ bookmark.title || '-' }}
        </DescriptionsItem>
        <DescriptionsItem :label="$t('bookmark.page.bookmark.description')">
          {{ bookmark.description || '-' }}
        </DescriptionsItem>
        <DescriptionsItem :label="$t('bookmark.page.bookmark.tags')">
          <template v-if="bookmark.tags && bookmark.tags.length">
            <Tag v-for="tag in bookmark.tags" :key="tag" color="blue">
              {{ tag }}
            </Tag>
          </template>
          <span v-else>-</span>
        </DescriptionsItem>
        <DescriptionsItem :label="$t('bookmark.page.bookmark.createdAt')">
          {{ bookmark.createTime || '-' }}
        </DescriptionsItem>
        <DescriptionsItem :label="$t('bookmark.page.bookmark.updatedAt')">
          {{ bookmark.updateTime || '-' }}
        </DescriptionsItem>
      </Descriptions>
    </template>

    <!-- Create/Edit Mode -->
    <template v-else-if="isCreateMode || isEditMode">
      <Form layout="vertical" :model="formState" @finish="handleSubmit">
        <FormItem
          :label="$t('bookmark.page.bookmark.url')"
          name="url"
          :rules="[{ required: true, message: $t('ui.formRules.required') }]"
        >
          <Input
            v-model:value="formState.url"
            :placeholder="$t('bookmark.page.bookmark.urlPlaceholder')"
            :maxlength="2048"
          />
        </FormItem>

        <FormItem
          :label="$t('bookmark.page.bookmark.name')"
          name="title"
          :rules="[{ required: true, message: $t('ui.formRules.required') }]"
        >
          <Input
            v-model:value="formState.title"
            :placeholder="$t('bookmark.page.bookmark.namePlaceholder')"
            :maxlength="255"
          />
        </FormItem>

        <FormItem
          :label="$t('bookmark.page.bookmark.description')"
          name="description"
        >
          <Textarea
            v-model:value="formState.description"
            :rows="4"
            :placeholder="
              $t('bookmark.page.bookmark.descriptionPlaceholder')
            "
          />
        </FormItem>

        <FormItem :label="$t('bookmark.page.bookmark.tags')" name="tags">
          <Select
            v-model:value="formState.tags"
            mode="tags"
            :placeholder="$t('bookmark.page.bookmark.tagsPlaceholder')"
          />
        </FormItem>

        <FormItem>
          <Space>
            <Button type="primary" html-type="submit" :loading="loading">
              {{
                isCreateMode
                  ? $t('ui.button.create', { moduleName: '' })
                  : $t('ui.button.save')
              }}
            </Button>
          </Space>
        </FormItem>
      </Form>
    </template>
  </Drawer>
</template>
