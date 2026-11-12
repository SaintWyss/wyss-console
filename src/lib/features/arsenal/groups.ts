/** Ordered classification from the approved Arsenal contract. Corrections stay local to a machine. */
export const GROUPS = [
  'models',
  'security',
  'debug',
  'content',
  'motion',
  'design',
  'study',
  'decide',
  'research',
  'review',
  'docs',
  'planning',
  'build',
  'system',
  'other',
] as const;
export type Group = (typeof GROUPS)[number];
export type Family = 'GSD' | 'gstack' | 'SEO' | 'HyperFrames' | null;

const rules: ReadonlyArray<readonly [Group, readonly string[]]> = [
  ['security', ['security', 'cso', 'guard', 'careful', 'freeze']],
  ['debug', ['debug', 'investigate']],
  ['content', ['content-strategy', 'social-media']],
  [
    'motion',
    [
      'gsap',
      'anime',
      'animation',
      'lottie',
      'three',
      'typegpu',
      'waapi',
      'hyperframes',
      'remotion',
      'contribute-catalog',
    ],
  ],
  [
    'design',
    [
      'design',
      '-ui',
      'ui-',
      'ux',
      'frontend',
      'tailwind',
      'shadcn',
      'accessibility',
      'image-to-code',
      'taste',
      'dashboard',
    ],
  ],
  ['study', ['tutor', 'quiz', 'cursos', 'second-brain', 'inbox', 'learn']],
  [
    'decide',
    ['devils-advocate', 'fool', 'junta', 'council', 'ceo-review', 'advisor'],
  ],
  [
    'research',
    [
      'research',
      'mapper',
      'graphify',
      'scrape',
      'discovery',
      'analyzer',
      'intel',
      'profiler',
    ],
  ],
  [
    'review',
    [
      'review',
      'verif',
      'checker',
      'auditor',
      'qa',
      'test',
      'eval',
      'health',
      'benchmark',
      'canary',
      'playwright',
    ],
  ],
  [
    'docs',
    [
      'doc-',
      'document',
      'markdown',
      'humanizer',
      'pdf',
      'prompt',
      'skill-creator',
      'skillify',
      'retro',
      'report',
    ],
  ],
  [
    'planning',
    [
      'plan',
      'roadmap',
      'spec',
      'office-hours',
      'framework-selector',
      'synthesizer',
      'curator',
    ],
  ],
  [
    'build',
    [
      'executor',
      'fixer',
      'ship',
      'deploy',
      'fastapi',
      'solid',
      'typescript',
      'react',
      'vite',
      'pair-agent',
    ],
  ],
  [
    'system',
    [
      'cachyos',
      'workstation',
      'doctor',
      'setup',
      'upgrade',
      'ios-',
      'context-',
      'sync-',
      'browser',
    ],
  ],
];

export function groupFor(
  item: { name: string; kind: string; description: string },
  correction?: string,
): Group {
  if (correction !== undefined)
    return (GROUPS as readonly string[]).includes(correction)
      ? (correction as Group)
      : 'other';
  const name = item.name.toLowerCase();
  if (
    item.kind === 'rule' ||
    (item.kind === 'agent' && /^(astra|luna|sol)_/.test(name))
  )
    return 'models';
  for (const [group, words] of rules) {
    if (group === 'content' && name.startsWith('seo-')) return group;
    if (
      group === 'planning' &&
      ['arsenal', 'combo', 'loops', 'orient'].includes(name)
    )
      return group;
    if (words.some((word) => name.includes(word))) return group;
    if (
      group === 'build' &&
      ['implement', 'build', 'scaffold'].some((word) =>
        item.description.toLowerCase().includes(word),
      )
    )
      return group;
  }
  return 'other';
}

export function familyFor(name: string): Family {
  const lower = name.toLowerCase();
  if (lower.startsWith('gsd-')) return 'GSD';
  if (lower.startsWith('gstack-')) return 'gstack';
  if (lower.startsWith('seo-')) return 'SEO';
  if (lower.startsWith('hyperframes')) return 'HyperFrames';
  return null;
}
