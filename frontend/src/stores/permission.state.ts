import { defineStore } from 'pinia';

import {
  BookmarkPermissionService,
  type CheckAccessResponse,
  type GetEffectivePermissionsResponse,
  type GrantAccessRequest,
  type GrantAccessResponse,
  type ListAccessibleResourcesResponse,
  type ListPermissionsResponse,
  type Permission,
  type Relation,
  type ResourceType,
  type SubjectType,
} from '../api/services';

export interface Paging {
  page?: number;
  pageSize?: number;
}

export const useBookmarkPermissionStore = defineStore(
  'bookmark-permission',
  () => {
    async function grantAccess(
      request: GrantAccessRequest,
    ): Promise<GrantAccessResponse> {
      return await BookmarkPermissionService.grant(request);
    }

    async function revokeAccess(
      resourceType: ResourceType,
      resourceId: string,
      subjectType: SubjectType,
      subjectId: string,
      relation?: Relation,
    ): Promise<void> {
      return await BookmarkPermissionService.revoke(
        resourceType,
        resourceId,
        subjectType,
        subjectId,
        relation,
      );
    }

    async function listPermissions(
      resourceType: ResourceType,
      resourceId: string,
      paging?: Paging,
    ): Promise<ListPermissionsResponse> {
      return await BookmarkPermissionService.list(
        resourceType,
        resourceId,
        { page: paging?.page, pageSize: paging?.pageSize },
      );
    }

    async function checkAccess(
      userId: string,
      resourceType: ResourceType,
      resourceId: string,
      permission: Permission,
    ): Promise<CheckAccessResponse> {
      return await BookmarkPermissionService.check({
        userId,
        resourceType,
        resourceId,
        permission,
      });
    }

    async function listAccessibleResources(
      userId: string,
      resourceType: ResourceType,
      permission: Permission,
      paging?: Paging,
    ): Promise<ListAccessibleResourcesResponse> {
      return await BookmarkPermissionService.listAccessible(
        userId,
        resourceType,
        permission,
        { page: paging?.page, pageSize: paging?.pageSize },
      );
    }

    async function getEffectivePermissions(
      userId: string,
      resourceType: ResourceType,
      resourceId: string,
    ): Promise<GetEffectivePermissionsResponse> {
      return await BookmarkPermissionService.getEffective(
        userId,
        resourceType,
        resourceId,
      );
    }

    function $reset() {}

    return {
      $reset,
      grantAccess,
      revokeAccess,
      listPermissions,
      checkAccess,
      listAccessibleResources,
      getEffectivePermissions,
    };
  },
);
