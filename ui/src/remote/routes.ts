import type { RouteRecordRaw } from 'vue-router'
import '@/main.css'

// Routes mounted by the platform shell under their own error boundary; the
// paths are the manifest's nav paths (src/manifest.rs nav()).
export const routes: RouteRecordRaw[] = [
  { path: '/bookmark', name: 'bookmark-list', component: () => import('@/views/bookmarks/index.vue'), meta: { module: 'bookmark' } },
  { path: '/bookmark/permissions', name: 'bookmark-permissions', component: () => import('@/views/permissions/index.vue'), meta: { module: 'bookmark' } },
  { path: '/bookmark/backup', name: 'bookmark-backup', component: () => import('@/views/backup/index.vue'), meta: { module: 'bookmark' } },
]
export default routes
