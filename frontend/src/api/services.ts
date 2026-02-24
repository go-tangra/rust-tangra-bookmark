/**
 * Bookmark Module Service Functions
 *
 * Typed service methods for the Bookmark API using dynamic module routing.
 * Base URL: /admin/v1/modules/bookmark/v1
 */

import { bookmarkApi, type RequestOptions } from './client';

// ==================== Entity Types ====================

export interface Bookmark {
  id: string;
  tenantId: number;
  url: string;
  title: string;
  description: string;
  tags: string[];
  createdBy?: number;
  createTime: string;
  updateTime?: string;
}

export type ResourceType =
  | 'RESOURCE_TYPE_UNSPECIFIED'
  | 'RESOURCE_TYPE_BOOKMARK';

export type Relation =
  | 'RELATION_UNSPECIFIED'
  | 'RELATION_OWNER'
  | 'RELATION_EDITOR'
  | 'RELATION_VIEWER'
  | 'RELATION_SHARER';

export type SubjectType =
  | 'SUBJECT_TYPE_UNSPECIFIED'
  | 'SUBJECT_TYPE_USER'
  | 'SUBJECT_TYPE_ROLE'
  | 'SUBJECT_TYPE_TENANT';

export type Permission =
  | 'PERMISSION_UNSPECIFIED'
  | 'PERMISSION_READ'
  | 'PERMISSION_WRITE'
  | 'PERMISSION_DELETE'
  | 'PERMISSION_SHARE';

export interface PermissionTuple {
  id: number;
  tenantId: number;
  resourceType: ResourceType;
  resourceId: string;
  relation: Relation;
  subjectType: SubjectType;
  subjectId: string;
  grantedBy?: number;
  expiresAt?: string;
  createTime: string;
}

// ==================== Request/Response Types ====================

export interface CreateBookmarkRequest {
  url: string;
  title: string;
  description?: string;
  tags?: string[];
}

export interface UpdateBookmarkRequest {
  url?: string;
  title?: string;
  description?: string;
  tags?: string[];
  updateTags?: boolean;
}

export interface ListBookmarksResponse {
  bookmarks: Bookmark[];
  total: number;
}

export interface GrantAccessRequest {
  resourceType: ResourceType;
  resourceId: string;
  relation: Relation;
  subjectType: SubjectType;
  subjectId: string;
  expiresAt?: string;
}

export interface GrantAccessResponse {
  permission: PermissionTuple;
}

export interface ListPermissionsResponse {
  permissions: PermissionTuple[];
  total: number;
}

export interface CheckAccessRequest {
  userId: string;
  resourceType: ResourceType;
  resourceId: string;
  permission: Permission;
}

export interface CheckAccessResponse {
  allowed: boolean;
  reason?: string;
}

export interface ListAccessibleResourcesResponse {
  resourceIds: string[];
  total: number;
}

export interface GetEffectivePermissionsResponse {
  permissions: Permission[];
  highestRelation: Relation;
}

// ==================== Helper ====================

function buildQuery(params: Record<string, unknown>): string {
  const searchParams = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== '') {
      searchParams.append(key, String(value));
    }
  }
  const query = searchParams.toString();
  return query ? `?${query}` : '';
}

// ==================== Bookmark Service ====================

export const BookmarkService = {
  create: (data: CreateBookmarkRequest, options?: RequestOptions) =>
    bookmarkApi.post<Bookmark>('/bookmarks', data, options),

  get: (id: string, options?: RequestOptions) =>
    bookmarkApi.get<Bookmark>(`/bookmarks/${id}`, options),

  list: (
    params?: {
      page?: number;
      pageSize?: number;
      tagFilter?: string;
    },
    options?: RequestOptions,
  ) => {
    const qs = buildQuery({
      page: params?.page,
      pageSize: params?.pageSize,
      tagFilter: params?.tagFilter,
    });
    return bookmarkApi.get<ListBookmarksResponse>(
      `/bookmarks${qs}`,
      options,
    );
  },

  update: (
    id: string,
    data: UpdateBookmarkRequest,
    options?: RequestOptions,
  ) => bookmarkApi.put<Bookmark>(`/bookmarks/${id}`, data, options),

  delete: (id: string, options?: RequestOptions) =>
    bookmarkApi.delete<void>(`/bookmarks/${id}`, options),
};

// ==================== Permission Service ====================

export const BookmarkPermissionService = {
  grant: (data: GrantAccessRequest, options?: RequestOptions) =>
    bookmarkApi.post<GrantAccessResponse>('/permissions', data, options),

  revoke: (
    resourceType: ResourceType,
    resourceId: string,
    subjectType: SubjectType,
    subjectId: string,
    relation?: Relation,
  ) =>
    bookmarkApi.delete<void>(
      `/permissions${buildQuery({
        resourceType,
        resourceId,
        subjectType,
        subjectId,
        relation,
      })}`,
    ),

  list: (
    resourceType: ResourceType,
    resourceId: string,
    params?: { page?: number; pageSize?: number },
    options?: RequestOptions,
  ) =>
    bookmarkApi.get<ListPermissionsResponse>(
      `/permissions${buildQuery({
        resourceType,
        resourceId,
        page: params?.page,
        pageSize: params?.pageSize,
      })}`,
      options,
    ),

  check: (data: CheckAccessRequest, options?: RequestOptions) =>
    bookmarkApi.post<CheckAccessResponse>(
      '/permissions/check',
      data,
      options,
    ),

  listAccessible: (
    userId: string,
    resourceType: ResourceType,
    permission: Permission,
    params?: { page?: number; pageSize?: number },
    options?: RequestOptions,
  ) =>
    bookmarkApi.get<ListAccessibleResourcesResponse>(
      `/permissions/accessible${buildQuery({
        userId,
        resourceType,
        permission,
        page: params?.page,
        pageSize: params?.pageSize,
      })}`,
      options,
    ),

  getEffective: (
    userId: string,
    resourceType: ResourceType,
    resourceId: string,
    options?: RequestOptions,
  ) =>
    bookmarkApi.get<GetEffectivePermissionsResponse>(
      `/permissions/effective${buildQuery({
        userId,
        resourceType,
        resourceId,
      })}`,
      options,
    ),
};
