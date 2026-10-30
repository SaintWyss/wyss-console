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
  Preview,
  Summary,
} from '../src/lib/contracts/arsenal';

let component: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';

const design: Summary = {
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
    notice: null,
    presets: [],
    selections: [],
    preview: null,
    chosenDetail: null,
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
      'Installed does not mean imported. Nothing here changes a session.',
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
      name: 'GSD Core 1.15.0',
      version: '1.15.0',
      artifacts: 1950,
      members: 1,
      origin: '~/.local/share/wyss/gsd-core/1.15.0',
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
    expect(text(root)).toContain('1950 artifacts in the package · read only');
    expect(text(root)).toContain('It is read only: GSD Core installs it');
    expect(buttonNamed(root, 'Import')?.disabled).toBe(true);
  });

  it('says when no project is selected, so project capabilities are not mistaken for absent ones', () => {
    const { root } = render(
      stateWith({ tab: 'discover', discover: discoverWith([]) }),
    );
    expect(text(root)).toContain('No project selected');
  });

  it('keeps a preset without a name from being saved and says what to fix', async () => {
    const controller = controllerMock();
    const { root } = render(stateWith({ tab: 'presets' }), controller);
    const form = root.querySelector<HTMLFormElement>(
      'form[aria-label="Preset editor"]',
    )!;
    form.dispatchEvent(
      new Event('submit', { cancelable: true, bubbles: true }),
    );
    flushSync();
    expect(controller.savePreset).not.toHaveBeenCalled();
    expect(root.querySelector('[role="alert"]')?.textContent).toContain(
      'Name the preset before saving.',
    );
  });

  it('labels a configuration preview NOT APPLIED, names instruction overlaps, and states that dependencies are not analysed', () => {
    const preview: Preview = {
      selection: {
        id: 'sel-1',
        name: 'Frontend',
        provider: 'codex',
        items: [],
        created_at: 1,
        updated_at: 1,
      },
      status: 'not_applied',
      items: [
        {
          capability: 'cap-design',
          state: 'resolved',
          kind: 'skill',
          name: 'design',
          provider: 'claude_code',
          revision: 2,
          pinned: false,
          tree_hash: 'c'.repeat(64),
          compatibility: 'unsupported',
          reason: 'Built for claude_code; this selection is for codex.',
        },
      ],
      conflicts: [
        {
          code: 'instruction_overlap',
          capabilities: ['cap-design', 'cap-brief'],
        },
      ],
      dependencies: 'not_analysed',
    };
    const { root } = render(stateWith({ tab: 'selections', preview }));
    expect(text(root)).toContain('NOT APPLIED');
    expect(text(root)).toContain('Dependencies: not analysed in this version.');
    expect(text(root)).toContain('Unsupported');
    expect(text(root)).toContain('Two rules for one provider');
  });
});
