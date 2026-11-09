import type { NavGroup } from './nav';

/** How the owner left the sidebar: collapsed to icons, and which groups are folded. */
export interface SidebarPrefs {
  rail: boolean;
  closed: NavGroup[];
}

const KEY = 'wyss-console.sidebar';
const GROUPS: readonly NavGroup[] = [
  'terminals',
  'agents',
  'capabilities',
  'system',
];

export const DEFAULT_SIDEBAR: SidebarPrefs = { rail: false, closed: [] };

/** Reads what `raw` holds. Anything that is not a valid record gives the default. */
export function parseSidebar(raw: string | null): SidebarPrefs {
  if (!raw) return { ...DEFAULT_SIDEBAR, closed: [] };
  try {
    const value: unknown = JSON.parse(raw);
    if (typeof value !== 'object' || value === null)
      return { ...DEFAULT_SIDEBAR, closed: [] };
    const { rail, closed } = value as Record<string, unknown>;
    return {
      rail: rail === true,
      closed: Array.isArray(closed)
        ? GROUPS.filter((group) => closed.includes(group))
        : [],
    };
  } catch {
    return { ...DEFAULT_SIDEBAR, closed: [] };
  }
}

/** Browser storage can be absent or refuse: the sidebar then simply starts open. */
export function loadSidebar(): SidebarPrefs {
  try {
    return parseSidebar(globalThis.localStorage?.getItem(KEY) ?? null);
  } catch {
    return { ...DEFAULT_SIDEBAR, closed: [] };
  }
}

export function saveSidebar(prefs: SidebarPrefs): void {
  try {
    globalThis.localStorage?.setItem(KEY, JSON.stringify(prefs));
  } catch {
    /* A preference that cannot be kept is not an error. */
  }
}
