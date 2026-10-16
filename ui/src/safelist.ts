// Icons outside the kit's set (@go-tangra/ui ICONS) that the pages use. The
// shell's stylesheet carries only the kit set and Tailwind emits only the class
// names it finds in the sources, so this remote's stylesheet carries these.
// Navigation icons must be in the kit set (the shell draws the menu):
// tests/unit/remote.spec.ts checks the manifest's nav icons.
export const EXTRA_ICONS = ['icon-[mdi--bookmark-plus-outline]'] as const
