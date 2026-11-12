import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import ArsenalView from '../src/lib/features/arsenal/ArsenalView.svelte';
import type {
  ArsenalController,
  ArsenalState,
} from '../src/lib/features/arsenal/controller';
import type {
  Candidate,
  Composite,
  LibraryItem,
} from '../src/lib/contracts/arsenal';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';

const design: LibraryItem = {
  id: 'cap-design',
  provider: 'claude_code',
  source: 'claude_user_skills',
  kind: 'skill',
  name: 'design',
  description: 'Plans screens',
  origin: '~/.claude/skills/design',
  revision: 2,
  revisions: 2,
  tree_hash: 'c'.repeat(64),
  updated_at: 1,
  compatibility: { status: 'supported', reason: 'own format' },
};

const candidate = (overrides: Partial<Candidate>): Candidate => ({
  id: 'id-x',
  tree_hash: 'f'.repeat(64),
  status: 'not_imported',
  importable: true,
  compatibility: 'unverified',
  capability: null,
  duplicates: [],
  name_collisions: [],
  kind: 'skill',
  provider: 'claude_code',
  source: 'claude_user_skills',
  scope: 'user',
  name: 'tokens',
  description: 'Design tokens',
  origin: '~/.claude/skills/tokens',
  file_count: 0,
  excluded_count: 0,
  review_count: 0,
  executes_code: false,
  blocked: null,
  managed_by: null,
  ...overrides,
});

function stateWith(overrides: Partial<ArsenalState>): ArsenalState {
  return {
    tab: 'library',
    phase: 'ready',
    library: [design],
    opened: null,
    file: null,
    discover: null,
    discovering: false,
    chosen: null,
    busy: null,
    bulk: null,
    notice: null,
    presets: [],
    selections: [],
    preview: null,
    chosenDetail: null,
    bindings: [],
    sync: null,
    groups: {},
    movingGroup: null,
    groupFailure: null,
    plan: null,
    selectionsFailure: null,
    ...overrides,
  };
}

function discoverWith(
  candidates: Candidate[],
  composites: Composite[] = [],
): NonNullable<ArsenalState['discover']> {
  return {
    project: null,
    sources: [{ source: 'claude_user_skills', state: 'read' }],
    candidates,
    composites,
    truncated: false,
  };
}

function controllerMock() {
  return {
    setTab: vi.fn(),
    checkDraft: vi.fn(),
    moveGroup: vi.fn(),
    choose: vi.fn(),
    importCandidate: vi.fn(),
    discover: vi.fn(),
    refresh: vi.fn(),
    open: vi.fn(),
    showFile: vi.fn(),
    restore: vi.fn(),
    clearNotice: vi.fn(),
    savePreset: vi.fn(async () => true),
    saveSelection: vi.fn(async () => true),
    deletePreset: vi.fn(),
    deleteSelection: vi.fn(),
    loadPreview: vi.fn(),
    loadBindings: vi.fn(),
  };
}

function render(state: ArsenalState, controller = controllerMock()) {
  component = mount(ArsenalView, {
    target: document.body,
    props: {
      visible: true,
      arsenal: state,
      controller: controller as unknown as ArsenalController,
    },
  });
  flushSync();
  return {
    root: document.querySelector<HTMLElement>('.arsenal-feature')!,
    controller,
  };
}

const buttonNamed = (root: HTMLElement, label: string) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].find(
    (button) => button.textContent?.trim() === label,
  );

describe('Arsenal view', () => {
  it('lists the library with its kind, provider, revision and compatibility, and says what it is not', () => {
    const { root } = render(stateWith({}));
    expect(text(root)).toContain(
      'Installed does not mean imported. A selection reaches a terminal only when you start one with it from Workspace.',
    );
    expect(text(root)).toContain('design');
    expect(text(root)).toContain('Skill');
    expect(text(root)).toContain('Revision 2');
    expect(text(root)).toContain('Unverified');
  });

  it('shows an unavailable store as an alert, never as an empty library', () => {
    const { root } = render(stateWith({ phase: 'unavailable', library: [] }));
    expect(root.querySelector('[role="alert"]')?.textContent).toContain(
      'could not be opened',
    );
    expect(text(root)).not.toContain('Nothing is imported yet');
  });

  it('opens the details of a capability through the controller', () => {
    const { root, controller } = render(stateWith({}));
    root.querySelector<HTMLButtonElement>('.arsenal-row')!.click();
    expect(controller.open).toHaveBeenCalledWith('cap-design');
  });

  it('marks an imported skill as imported, and blocks a skill that cannot be imported, saying why', () => {
    const { root, controller } = render(
      stateWith({
        tab: 'discover',
        discover: discoverWith([
          candidate({ id: 'id-tokens', status: 'imported', name: 'tokens' }),
          candidate({
            id: 'id-leaky',
            name: 'leaky',
            importable: false,
            blocked: 'needs_secret',
          }),
        ]),
        chosenDetail: {
          id: 'id-leaky',
          files: [],
          excluded: [{ path: 'credentials.json', reason: 'credential_name' }],
          review: [],
        },
        chosen: 'id-leaky',
      }),
    );
    expect(text(root)).toContain('Imported');
    expect(text(root)).toContain('Credential file, not copied');
    expect(buttonNamed(root, 'Import')?.disabled).toBe(true);
    expect(root.querySelector('[role="alert"]')?.textContent).toContain(
      'looks like a secret',
    );
    root.querySelector<HTMLButtonElement>('[aria-pressed="false"]')?.click();
    expect(controller.choose).toHaveBeenCalledWith('id-tokens');
  });

  it('lists a project in a folder another account can write read only, and says why it is not copied', () => {
    const { root } = render(
      stateWith({
        tab: 'discover',
        discover: discoverWith([
          candidate({
            id: 'id-claude',
            kind: 'rule',
            name: 'CLAUDE',
            scope: 'project',
            source: 'claude_project_rules',
            importable: false,
            blocked: 'shared_path',
          }),
        ]),
        chosen: 'id-claude',
      }),
    );
    expect(root.querySelector('[role="alert"]')?.textContent).toContain(
      'another account can write',
    );
    expect(buttonNamed(root, 'Import')?.disabled).toBe(true);
    // Nothing is copied from it, so the file list must not promise a copy.
    expect(text(root)).toContain('Files in this capability');
    expect(text(root)).not.toContain('Files that will be copied');
  });

  it('offers a new revision when the source changed, and never claims the session uses it', () => {
    const { root } = render(
      stateWith({
        tab: 'discover',
        discover: discoverWith([
          candidate({ id: 'id-tokens', status: 'source_changed' }),
        ]),
        chosen: 'id-tokens',
      }),
    );
    expect(text(root)).toContain('Import as new revision');
    expect(text(root)).not.toContain('active');
  });

  it('lists files that use a credential word for review, and still allows the import', () => {
    const { root } = render(
      stateWith({
        tab: 'discover',
        discover: discoverWith([
          candidate({
            id: 'id-prose',
            name: 'prose',
          }),
        ]),
        chosenDetail: {
          id: 'id-prose',
          files: [{ path: 'SKILL.md', size: 12, sha256: 'a'.repeat(64) }],
          excluded: [],
          review: [{ path: 'SKILL.md', reason: 'mentions_credential' }],
        },
        chosen: 'id-prose',
      }),
    );
    expect(text(root)).toContain('Review before importing');
    expect(text(root)).toContain('Mentions a credential word');
    expect(buttonNamed(root, 'Import')?.disabled).toBe(false);
  });

  it('shows a hook as read only: named, explained, and with no import action', () => {
    const { root } = render(
      stateWith({
        tab: 'discover',
        discover: discoverWith([
          candidate({
            id: 'id-hook',
            kind: 'hook',
            name: 'pre-commit',
            status: 'read_only',
            importable: false,
            compatibility: 'unsupported',
            blocked: 'read_only_kind',
          }),
        ]),
        chosen: 'id-hook',
      }),
    );
    expect(text(root)).toContain(
      'A hook runs code. Arsenal lists it and never copies or runs it.',
    );
    expect(buttonNamed(root, 'Import')).toBeUndefined();
  });

  it('shows a composite collection and a link into it as read only, not as a blocked skill', () => {
    const composite: Composite = {
      key: 'gsd_core@1.15.0',
      kind: 'gsd_core',
      provider: 'claude_code',
      name: 'GSD Core 1.15.0',
      version: '1.15.0',
      description: 'Spec-driven workflow.',
      artifacts: 1950,
      members: 1,
      origin: '~/.local/share/wyss/gsd-core/1.15.0',
      enabled: null,
      methodology: true,
      skills: ['gsd-help'],
      agents: [],
      commands: 0,
      hooks: false,
      mcp: false,
      categories: [['skill', 216]],
      limitations: ['Installed by GSD Core as links.'],
    };
    const { root } = render(
      stateWith({
        tab: 'discover',
        discover: discoverWith(
          [
            candidate({
              id: 'id-gsd',
              name: 'gsd-help',
              importable: false,
              blocked: 'symbolic_link',
              managed_by: {
                composite: 'GSD Core',
                version: '1.15.0',
                artifacts: 1950,
              },
            }),
          ],
          [composite],
        ),
        chosen: 'id-gsd',
      }),
    );
    expect(text(root)).toContain('GSD Core 1.15.0');
    expect(text(root)).toContain('1 composite collection(s) installed.');
    expect(text(root)).toContain('It is read only: GSD Core installs it');
    expect(buttonNamed(root, 'Import')?.disabled).toBe(true);
  });

  it('names a Codex plugin as one, with its switch and what it brings, and never as importable', () => {
    const plugin: Composite = {
      key: 'codex_plugin:pdf@runtime',
      kind: 'codex_plugin',
      provider: 'codex',
      name: 'pdf',
      version: '1.2.3',
      description: 'Reads PDFs',
      artifacts: 0,
      members: 1,
      origin: '~/.codex/plugins/cache/runtime/pdf/1.2.3',
      enabled: true,
      methodology: false,
      skills: ['pdf'],
      agents: [],
      commands: 0,
      hooks: false,
      mcp: true,
      categories: [],
      limitations: ['It declares MCP servers.'],
    };
    const { root } = render(
      stateWith({ tab: 'composites', discover: discoverWith([], [plugin]) }),
    );
    const section = root.querySelector('section[aria-label="pdf"]');
    expect(text(section)).toContain('Codex plugin');
    expect(text(section)).toContain('Enabled in your settings');
    expect(text(section)).toContain('It declares MCP servers.');
    expect(text(section)).toContain('Not importable');
  });

  it('says when no project is selected, so project capabilities are not mistaken for absent ones', () => {
    const { root } = render(
      stateWith({ tab: 'discover', discover: discoverWith([]) }),
    );
    expect(text(root)).toContain('No project selected');
  });
});

describe('Arsenal composites and terminals', () => {
  it('describes a composite with its source, members, hooks and limits, never as importable', () => {
    const plugin: Composite = {
      key: 'claude_plugin:superpowers@official',
      kind: 'claude_plugin',
      provider: 'claude_code',
      name: 'superpowers',
      version: '7.0.0',
      description: 'Core skills library',
      artifacts: 0,
      members: 14,
      origin: '~/.claude/plugins/cache/official/superpowers/7.0.0',
      enabled: false,
      methodology: true,
      skills: ['brainstorming', 'systematic-debugging'],
      agents: [],
      commands: 0,
      hooks: true,
      mcp: false,
      categories: [],
      limitations: [
        'Plugin skills are not switched off by skillOverrides; a plugin is enabled or disabled as a whole.',
      ],
    };
    const { root } = render(
      stateWith({
        tab: 'composites',
        discover: {
          project: null,
          sources: [],
          candidates: [],
          composites: [plugin],
          truncated: false,
        },
      }),
    );
    const shown = text(root);
    expect(shown).toContain('Claude Code plugin');
    expect(shown).toContain('Version 7.0.0');
    expect(shown).toContain('Disabled in your settings');
    expect(shown).toContain('Not importable');
    expect(shown).toContain('Yes: it registers hooks.');
    expect(shown).toContain('not switched off by skillOverrides');
    expect(buttonNamed(root, 'Import')).toBeUndefined();
  });

  it('lists what terminals started with, applied or not, and never as verified', () => {
    const { root } = render(
      stateWith({
        tab: 'activity',
        bindings: [
          {
            session_id: 'a'.repeat(32),
            selection_id: 'sel-1',
            selection_name: 'Frontend',
            provider: 'claude_code',
            provider_version: '2.1.314 (Claude Code)',
            project: '/home/u/app',
            config_hash: 'e'.repeat(64),
            state: 'not_applied',
            reason: 'Skill exclusion is not qualified for Claude Code 2.1.314.',
            plan: null,
            created_at: 1,
            verification: 'not_observed',
          },
        ],
      }),
    );
    const shown = text(root);
    expect(shown).toContain('NOT APPLIED');
    expect(shown).toContain('Use not observed');
    expect(shown).toContain('not qualified for Claude Code 2.1.314');
    expect(shown).not.toContain('Verified');
  });
});

describe('Arsenal sections', () => {
  const sourcesWith = (
    sources: { source: string; state: 'read' | 'absent' | 'unreadable' }[],
  ) =>
    ({ ...discoverWith([]), sources }) as NonNullable<ArsenalState['discover']>;
  const radio = (root: Element, name: string) =>
    [...root.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find(
      (button) => text(button).startsWith(name),
    )!;

  it('has three tabs, Presets, Library and Activity, and opens on Presets', () => {
    const { root } = render(stateWith({ tab: 'presets' }));
    expect(
      [...root.querySelectorAll('[role="tab"]')].map((tab) => text(tab)),
    ).toEqual(['Presets', 'Library', 'Activity']);
    expect(
      root.querySelector('#arsenal-tab-presets')?.getAttribute('aria-selected'),
    ).toBe('true');
    expect(text(root)).toContain('New preset');
  });

  it('keeps Library selected while Discover or Composites is open, and switches between them through the controller', () => {
    const controller = controllerMock();
    const { root } = render(
      stateWith({ tab: 'discover', discover: discoverWith([]) }),
      controller,
    );
    expect(
      root.querySelector('#arsenal-tab-library')?.getAttribute('aria-selected'),
    ).toBe('true');
    expect(
      root
        .querySelector('#arsenal-section-discover')
        ?.getAttribute('aria-current'),
    ).toBe('true');
    root
      .querySelector<HTMLButtonElement>('#arsenal-section-composites')!
      .click();
    expect(controller.setTab).toHaveBeenCalledWith('composites');
  });

  it('disables a provider the scan found no source for, and says Not installed', () => {
    const { root } = render(
      stateWith({
        tab: 'presets',
        discover: sourcesWith([
          { source: 'claude_user_skills', state: 'read' },
          { source: 'codex_user_skills', state: 'absent' },
          { source: 'codex_home_skills', state: 'absent' },
        ]),
      }),
    );
    expect(radio(root, 'Codex').disabled).toBe(true);
    expect(text(radio(root, 'Codex'))).toContain('Not installed');
    expect(radio(root, 'Claude Code').disabled).toBe(false);
  });

  it('keeps every provider enabled until a scan has answered', () => {
    const { root } = render(stateWith({ tab: 'presets', discover: null }));
    expect(radio(root, 'Codex').disabled).toBe(false);
    expect(radio(root, 'Claude Code').disabled).toBe(false);
  });

  it('chooses the provider the tabs work on, and says which one is chosen', () => {
    const { root } = render(stateWith({ tab: 'presets' }));
    expect(radio(root, 'Claude Code').getAttribute('aria-checked')).toBe(
      'true',
    );
    radio(root, 'Codex').click();
    flushSync();
    expect(radio(root, 'Codex').getAttribute('aria-checked')).toBe('true');
    expect(radio(root, 'Claude Code').getAttribute('aria-checked')).toBe(
      'false',
    );
  });

  it('asks before switching provider with unsaved changes; Keep editing stays, Discard changes switches and empties the draft', () => {
    const { root } = render(stateWith({ tab: 'presets' }));
    root.querySelector<HTMLButtonElement>('.group-toggle')!.click();
    flushSync();
    const design = () =>
      root.querySelector<HTMLInputElement>('input[aria-label="design"]')!;
    design().checked = true;
    design().dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();

    radio(root, 'Codex').click();
    flushSync();
    expect(radio(root, 'Claude Code').getAttribute('aria-checked')).toBe(
      'true',
    );
    expect(radio(root, 'Codex').getAttribute('aria-checked')).toBe('false');
    expect(text(root.querySelector('.notice'))).toContain('Unsaved changes');

    buttonNamed(root, 'Keep editing')!.click();
    flushSync();
    expect(radio(root, 'Claude Code').getAttribute('aria-checked')).toBe(
      'true',
    );
    expect(design().checked).toBe(true);

    radio(root, 'Codex').click();
    flushSync();
    buttonNamed(root, 'Discard changes')!.click();
    flushSync();
    expect(radio(root, 'Codex').getAttribute('aria-checked')).toBe('true');
    expect(root.querySelectorAll('.cell[data-filled="true"]')).toHaveLength(0);
    expect(text(root.querySelector('.totals'))).toBe(
      'Nothing chosen yet. Tick a group or a capability.',
    );
  });

  it('switches provider without asking when the draft has no changes', () => {
    const { root } = render(stateWith({ tab: 'presets' }));
    radio(root, 'Codex').click();
    flushSync();
    expect(radio(root, 'Codex').getAttribute('aria-checked')).toBe('true');
    expect(root.querySelector('.notice')).toBeNull();
  });
});
