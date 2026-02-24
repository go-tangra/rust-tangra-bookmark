import type { TangraModule } from './sdk';

import routes from './routes';
import { useBookmarkStore } from './stores/bookmark.state';
import { useBookmarkPermissionStore } from './stores/permission.state';
import enUS from './locales/en-US.json';

const bookmarkModule: TangraModule = {
  id: 'bookmark',
  version: '1.0.0',
  routes,
  stores: {
    'bookmark-bookmark': useBookmarkStore,
    'bookmark-permission': useBookmarkPermissionStore,
  },
  locales: {
    'en-US': enUS,
  },
};

export default bookmarkModule;
