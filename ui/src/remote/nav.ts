/** Dynamic navigation entries (none: the manifest declares the static ones, src/manifest.rs nav()). */
export interface NavEntry {
  title: string
  path: string
  icon?: string
  order: number
}

export function nav(): NavEntry[] {
  return []
}
export default nav
