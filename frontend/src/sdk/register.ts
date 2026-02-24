import type { ShellContext, TangraModule } from './types';

export function registerModule(ctx: ShellContext, module: TangraModule) {
  // Remove any existing routes that overlap with this module's routes.
  // The backend menu discovery creates component-less stubs (e.g. "bookmark-list")
  // that conflict with the real routes from the federated module ("BookmarkList").
  // We must remove ALL matching routes by path before adding the module's routes.
  for (const route of module.routes) {
    const pathsToRemove = new Set<string>();
    pathsToRemove.add(route.path);
    if (route.children) {
      for (const child of route.children) {
        const childPath = child.path.startsWith('/')
          ? child.path
          : `${route.path}/${child.path}`;
        pathsToRemove.add(childPath);
      }
    }

    for (const existing of ctx.router.getRoutes()) {
      if (pathsToRemove.has(existing.path) && existing.name) {
        ctx.router.removeRoute(existing.name);
      }
    }

    ctx.router.addRoute(route);
  }

  // Merge i18n messages
  for (const [lang, messages] of Object.entries(module.locales)) {
    ctx.i18n.global.mergeLocaleMessage(lang, { [module.id]: messages });
  }
}
