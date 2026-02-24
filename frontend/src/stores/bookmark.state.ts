import { defineStore } from 'pinia';

import {
  BookmarkService,
  type Bookmark,
  type CreateBookmarkRequest,
  type ListBookmarksResponse,
  type UpdateBookmarkRequest,
} from '../api/services';

export const useBookmarkStore = defineStore('bookmark-bookmark', () => {
  async function listBookmarks(
    paging?: { page?: number; pageSize?: number },
    filters?: { tagFilter?: string } | null,
  ): Promise<ListBookmarksResponse> {
    return await BookmarkService.list({
      page: paging?.page,
      pageSize: paging?.pageSize,
      tagFilter: filters?.tagFilter,
    });
  }

  async function getBookmark(id: string): Promise<Bookmark> {
    return await BookmarkService.get(id);
  }

  async function createBookmark(
    data: CreateBookmarkRequest,
  ): Promise<Bookmark> {
    return await BookmarkService.create(data);
  }

  async function updateBookmark(
    id: string,
    data: UpdateBookmarkRequest,
  ): Promise<Bookmark> {
    return await BookmarkService.update(id, data);
  }

  async function deleteBookmark(id: string): Promise<void> {
    return await BookmarkService.delete(id);
  }

  function $reset() {}

  return {
    $reset,
    listBookmarks,
    getBookmark,
    createBookmark,
    updateBookmark,
    deleteBookmark,
  };
});
