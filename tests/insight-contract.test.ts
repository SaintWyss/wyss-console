import { describe, expect, it } from 'vitest';
import { parseInsight, parseReadmeName } from '../src/lib/contracts/insight';

function insight(over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    name: 'alpha',
    files: 3,
    folders: 2,
    bytes: 120,
    truncated: false,
    languages: [{ name: 'TypeScript', files: 2, bytes: 100 }],
    technologies: [
      {
        name: 'React',
        category: 'framework',
        version: '18.3.1',
        source: 'package.json',
      },
    ],
    architecture: [
      {
        name: 'Full-stack',
        description: 'Frontend and backend in one project.',
        confidence: 80,
        evidence: ['frontend and backend frameworks'],
      },
    ],
    structure: [{ name: 'src', dir: true, files: 2 }],
    manifests: ['package.json'],
    scripts: [{ source: 'package.json', name: 'dev', command: 'vite' }],
    git: true,
    license: null,
    readme: { name: 'README.md', content: '# Hello\n', truncated: false },
    types: [
      {
        name: 'User',
        kind: 'class',
        file: 'src/user.ts',
        language: 'TypeScript',
        extends: ['Base'],
        implements: ['Named'],
        fields: ['name: string'],
        methods: ['greet()'],
      },
    ],
    ...over,
  };
}

describe('what the desktop says about a project folder is checked, not believed', () => {
  it('reads the shape projects_insight sends', () => {
    const parsed = parseInsight(insight());
    expect(parsed.technologies[0].version).toBe('18.3.1');
    expect(parsed.readme?.content).toBe('# Hello\n');
    expect(parsed.types[0].kind).toBe('class');
  });

  it('accepts a folder without README or version', () => {
    const parsed = parseInsight(
      insight({
        readme: null,
        technologies: [
          { name: 'Vite', category: 'tooling', version: null, source: 'x' },
        ],
      }),
    );
    expect(parsed.readme).toBeNull();
    expect(parsed.technologies[0].version).toBeNull();
  });

  it('refuses an answer that is not an object', () => {
    expect(() => parseInsight(null)).toThrow(/projects/);
    expect(() => parseInsight('insight')).toThrow(/projects/);
  });

  it('refuses a negative or fractional count', () => {
    expect(() => parseInsight(insight({ files: -1 }))).toThrow(/projects/);
    expect(() => parseInsight(insight({ bytes: 1.5 }))).toThrow(/projects/);
  });

  it('refuses an unknown technology category or type kind', () => {
    expect(() =>
      parseInsight(
        insight({
          technologies: [
            { name: 'X', category: 'magic', version: null, source: 'x' },
          ],
        }),
      ),
    ).toThrow(/projects/);
    expect(() =>
      parseInsight(
        insight({
          types: [
            {
              name: 'X',
              kind: 'type',
              file: 'x.ts',
              language: 'TypeScript',
              extends: [],
              implements: [],
              fields: [],
              methods: [],
            },
          ],
        }),
      ),
    ).toThrow(/projects/);
  });

  it('refuses a confidence above 100', () => {
    expect(() =>
      parseInsight(
        insight({
          architecture: [
            { name: 'X', description: 'x', confidence: 101, evidence: [] },
          ],
        }),
      ),
    ).toThrow(/projects/);
  });

  it('refuses a readme with a non-text content', () => {
    expect(() =>
      parseInsight(
        insight({
          readme: { name: 'README.md', content: 1, truncated: false },
        }),
      ),
    ).toThrow(/projects/);
  });

  it('accepts only a text name for the README that was written', () => {
    expect(parseReadmeName('README.md')).toBe('README.md');
    expect(() => parseReadmeName(null)).toThrow(/projects/);
  });
});
