import { describe, expect, it } from 'vitest';
import {
  GROUPS,
  familyFor,
  groupFor,
  type Group,
} from '../src/lib/features/arsenal/groups';

const item = (name: string, kind = 'skill', description = '') => ({
  name,
  kind,
  description,
});

/** One name for each group, each matched by its own rule and by no earlier one. */
const representatives: [Group, ReturnType<typeof item>][] = [
  ['models', item('luna_max', 'agent')],
  ['security', item('cso')],
  ['debug', item('investigate')],
  ['content', item('social-media')],
  ['motion', item('gsap-react')],
  ['design', item('frontend-design')],
  ['study', item('quiz-maker')],
  ['decide', item('devils-advocate')],
  ['research', item('graphify')],
  ['review', item('playwright')],
  ['docs', item('pdf')],
  ['planning', item('roadmap')],
  ['build', item('vite-app')],
  ['system', item('cachyos-setup')],
  ['other', item('zzz-unmatched')],
];

describe('Arsenal group rules', () => {
  it('uses the first matching rule and handles special names', () => {
    expect(groupFor(item('security-review'))).toBe('security');
    expect(groupFor(item('seo-review'))).toBe('content');
    expect(groupFor(item('astra_high', 'agent'))).toBe('models');
    expect(groupFor(item('something', 'rule'))).toBe('models');
    expect(groupFor(item('orient'))).toBe('planning');
    expect(groupFor(item('unusual', 'skill', 'Scaffold a project'))).toBe(
      'build',
    );
    expect(groupFor(item('unusual'))).toBe('other');
  });
  it('uses a correction first and displays unknown corrections in Other', () => {
    expect(groupFor(item('security-review'), 'docs')).toBe('docs');
    expect(groupFor(item('security-review'), 'future-group')).toBe('other');
  });
  it('places each representative name in its own group', () => {
    for (const [group, named] of representatives) {
      expect(groupFor(named), named.name).toBe(group);
    }
  });
  it('gives every catalogue entry exactly one group, none empty, few in Other, the same way twice', () => {
    // A stand-in for the owner's library: the contract's measurement runs on the real one locally.
    const catalogue = [
      ...representatives.map(([, named]) => named),
      item('gsd-plan-phase'),
      item('security-audit'),
      item('seo-audit'),
      item('skill-creator'),
    ];
    const first = catalogue.map((named) => groupFor(named));
    const second = catalogue.map((named) => groupFor(named));
    expect(second).toEqual(first);
    expect(first.every((group) => GROUPS.includes(group))).toBe(true);
    expect(new Set(first)).toEqual(new Set(GROUPS));
    const other = first.filter((group) => group === 'other').length;
    expect(other / catalogue.length).toBeLessThanOrEqual(0.1);
  });
  it('classifies families independently of groups', () => {
    expect(familyFor('GSD-review')).toBe('GSD');
    expect(familyFor('gstack-qa')).toBe('gstack');
    expect(familyFor('seo-content')).toBe('SEO');
    expect(familyFor('hyperframes')).toBe('HyperFrames');
    expect(familyFor('other')).toBeNull();
  });
});
