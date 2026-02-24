import type { RouteRecordRaw } from 'vue-router';

const routes: RouteRecordRaw[] = [
  {
    path: '/bookmark',
    name: 'Bookmark',
    component: () => import('shell/app-layout'),
    redirect: '/bookmark/list',
    meta: {
      order: 2050,
      icon: 'lucide:bookmark',
      title: 'bookmark.menu.bookmark',
      keepAlive: true,
      authority: ['platform:admin', 'tenant:manager'],
    },
    children: [
      {
        path: 'list',
        name: 'BookmarkList',
        meta: {
          icon: 'lucide:list',
          title: 'bookmark.menu.list',
          authority: ['platform:admin', 'tenant:manager'],
        },
        component: () => import('./views/list/index.vue'),
      },
      {
        path: 'permission',
        name: 'BookmarkPermissions',
        meta: {
          icon: 'lucide:shield',
          title: 'bookmark.menu.permissions',
          authority: ['platform:admin', 'tenant:manager'],
        },
        component: () => import('./views/permissions/index.vue'),
      },
    ],
  },
];

export default routes;
