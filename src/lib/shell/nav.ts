export type ViewId =
  'console' | 'sessions' | 'agents' | 'approvals' | 'events' | 'settings';

export interface NavItem {
  id: ViewId;
  label: string;
  /** Shown under the view title in the top bar. */
  subtitle: string;
  /** SVG path data on a 24×24 grid, stroke-only. */
  icon: string;
  /** Placeholders stay visible but disabled until their feature exists. */
  available: boolean;
}

export const NAV_ITEMS: readonly NavItem[] = [
  {
    id: 'console',
    label: 'Console',
    subtitle: 'Local terminal workspace',
    icon: 'M4 7l4 4-4 4M11 16h7',
    available: true,
  },
  {
    id: 'sessions',
    label: 'Sessions',
    subtitle: 'Managed agent sessions',
    icon: 'M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5',
    available: true,
  },
  {
    id: 'agents',
    label: 'Agents',
    subtitle: 'Managed turns',
    icon: 'M5 8h14v11H5zM12 4v4M9.5 13h.01M14.5 13h.01',
    available: true,
  },
  {
    id: 'approvals',
    label: 'Approvals',
    subtitle: 'Actions waiting for a decision',
    icon: 'M12 3l7.5 3v5.5c0 4.5-3.2 8.2-7.5 9.5-4.3-1.3-7.5-5-7.5-9.5V6L12 3zM8.8 12.2l2.2 2.2 4.2-4.4',
    available: true,
  },
  {
    id: 'events',
    label: 'Events',
    subtitle: 'Coming soon',
    icon: 'M3 12h4l2.5-6 4 12 2.5-6H21',
    available: false,
  },
  {
    id: 'settings',
    label: 'Settings',
    subtitle: 'Coming soon',
    icon: 'M4 7h9M17 7h3M4 17h3M11 17h9M13 7a2 2 0 1 0 4 0 2 2 0 1 0-4 0M7 17a2 2 0 1 0 4 0 2 2 0 1 0-4 0',
    available: false,
  },
];
