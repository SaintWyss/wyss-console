import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import ProjectsHarness from './ProjectsHarness.svelte';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';
import {
  ASSOCIATION_1,
  BINDING_A,
  LEAF_1,
  PROJECT_A,
  SCRATCH,
  WORKSPACE_A,
  failure,
  fakeClient,
  overview,
  project,
  session,
  summary,
  workspace,
  type Fake,
} from './projects-harness';

const fakes = vi.hoisted(() => ({ factory: vi.fn() }));
vi.mock('../src/lib/features/terminal/renderer', () => ({
  createRenderer: fakes.factory,
}));

interface FakeRenderer extends TerminalRenderer {
  written: Uint8Array[];
  typing: { text: (d: string) => void; binary: (d: string) => void }[];
  inputEnabled: boolean;
}
let renderers: FakeRenderer[] = [];

let component: ReturnType<typeof mount> | undefined;
beforeEach(() => {
  renderers = [];
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  fakes.factory.mockImplementation(() => {
    const r: FakeRenderer = {
      written: [],
      typing: [],
      inputEnabled: false,
      fit: () => ({ rows: 24, cols: 80 }),
      write: async (data: Uint8Array) => {
        r.written.push(data);
        return true;
      },
      input: (text, binary) => {
        const mine = { text, binary };
        r.typing.push(mine);
        return () => {
          const at = r.typing.indexOf(mine);
          if (at >= 0) r.typing.splice(at, 1);
        };
      },
      setInput: (enabled: boolean) => {
        r.inputEnabled = enabled;
      },
      setActive: () => undefined,
      focus: () => undefined,
      dispose: () => undefined,
    };
    renderers.push(r);
    return r;
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  if (component) unmount(component);
  component = undefined;
});

const settle = async () => {
  for (let i = 0; i < 60; i++) await Promise.resolve();
  flushSync();
  await new Promise((resolve) => setTimeout(resolve, 12));
  for (let i = 0; i < 30; i++) await Promise.resolve();
  flushSync();
};
const text = (node: Element | null | undefined) =>
  node?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const button = (label: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((b) =>
    text(b).includes(label),
  );
const click = async (label: string) => {
  const found = button(label);
  if (!found) throw new Error(`no button "${label}"`);
  found.click();
  await settle();
};

async function shown(client: Fake) {
  component = mount(ProjectsHarness, {
    target: document.body,
    props: { client },
  });
  await settle();
}

const readyClient = () => {
  const client = fakeClient();
  return client;
};

describe('when there is nothing to show yet', () => {
  it('says plainly that the workspace is off and reads nothing else', async () => {
    const client = fakeClient();
    client.state.overview = overview({ enabled: false });
    await shown(client);
    expect(text(document.body)).toContain('Projects and workspaces are off');
    expect(client.projects).not.toHaveBeenCalled();
  });

  it('offers a preview of the import, or an empty start, when there is no store', async () => {
    const client = fakeClient();
    client.state.overview = overview({ store: { state: 'missing' } });
    await shown(client);
    expect(button('Preview the import')).toBeDefined();
    expect(button('Start empty instead')).toBeDefined();
    expect(client.execute).not.toHaveBeenCalled();
    await click('Preview the import');
    expect(text(document.querySelector('.projects-migration'))).toContain(
      '2 projects',
    );
    expect(client.commitMigration).not.toHaveBeenCalled();
    await click('Import');
    expect(client.commitMigration).toHaveBeenCalledWith(
      summary().previewId,
      false,
    );
  });

  it('does not offer to import what the preview says cannot be committed, and needs the alias acceptance', async () => {
    const client = fakeClient();
    client.state.overview = overview({ store: { state: 'missing' } });
    client.previewMigration = vi.fn(async () =>
      summary({
        canCommit: false,
        conflicts: [{ kind: 'source_unreadable', count: 1 }],
      }),
    );
    await shown(client);
    await click('Preview the import');
    expect(button('Import')!.disabled).toBe(true);
    expect(text(document.querySelector('.projects-conflicts'))).toContain(
      'cannot be read',
    );
    // An alias can be accepted, but only explicitly.
    client.previewMigration = vi.fn(async () =>
      summary({ canCommit: false, needsAliasAcceptance: true }),
    );
    await click('Cancel');
    await click('Preview the import');
    expect(button('Import')!.disabled).toBe(true);
    const box = document.querySelector<HTMLInputElement>(
      '.projects-migration input[type="checkbox"]',
    )!;
    box.click();
    await settle();
    expect(button('Import')!.disabled).toBe(false);
    await click('Import');
    expect(client.commitMigration).toHaveBeenCalledWith(
      summary().previewId,
      true,
    );
  });

  it('shows a store it cannot read as read-only and offers nothing to change it', async () => {
    const client = fakeClient();
    client.state.overview = overview({
      store: { state: 'unusable', code: 'unsupported_schema', field: 'schema' },
    });
    await shown(client);
    const body = text(document.body);
    expect(body).toContain('read-only');
    expect(body).toContain('newer version');
    expect(button('Add a folder')).toBeUndefined();
    expect(button('Start empty')).toBeUndefined();
  });
});

describe('a store that is ready', () => {
  it('lists Scratch and the projects and shows the workspace of the one in view', async () => {
    const client = readyClient();
    await shown(client);
    const items = Array.from(document.querySelectorAll('.projects-item')).map(
      (i) => text(i),
    );
    expect(items[0]).toContain('Scratch');
    expect(items[1]).toContain('Alpha');
    await click('Alpha');
    expect(client.execute).not.toHaveBeenCalled();
    expect(text(document.querySelector('.projects-canvas'))).toContain('Main');
    expect(text(document.querySelector('.projects-empty-pane'))).toContain(
      'Nothing runs here',
    );
  });

  it('says so when no terminal host answers, and offers nothing that would start one', async () => {
    const client = readyClient();
    client.state.overview = overview({
      host: {
        reachable: false,
        epoch: null,
        features: [],
        reason: 'host_offline',
      },
    });
    await shown(client);
    expect(text(document.body)).toContain('No terminal host answers');
    expect(text(document.body)).toContain('nothing is started or recreated');
  });

  it('shows a refusal, and Dismiss clears it', async () => {
    const client = readyClient();
    await shown(client);
    await click('Alpha');
    client.state.executeError = failure('root_missing');
    await click('Rename');
    const input = document.querySelector<HTMLInputElement>(
      'input[aria-label="Project name"]',
    )!;
    input.value = 'Gamma';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
    await click('Save name');
    expect(text(document.querySelector('.projects-banner'))).toContain(
      'The folder is missing',
    );
    await click('Dismiss');
    expect(document.querySelector('.projects-banner')).toBeNull();
  });
});

describe('starting something in an empty pane', () => {
  async function opened(client: Fake) {
    await shown(client);
    await click('Alpha');
    await click('Start…');
  }

  it('starts a shell by default and sends exactly that', async () => {
    const client = readyClient();
    await opened(client);
    await click('Start');
    const command = client.sent[0]!.command;
    expect(command).toMatchObject({
      kind: 'launch',
      provider: 'shell',
      mode: 'native',
      nativeProfile: null,
      managedSafetyProfile: null,
    });
  });

  it('never preselects a Managed profile, and Local tools is chosen explicitly', async () => {
    const client = readyClient();
    await opened(client);
    const provider = document.querySelector<HTMLSelectElement>(
      'select[aria-label="What to start"]',
    )!;
    provider.value = 'codex';
    provider.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    const managed = Array.from(
      document.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    ).find((r) => r.value === 'managed')!;
    managed.click();
    await settle();
    const radios = Array.from(
      document.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    );
    const profiles = radios.filter(
      (r) => r.value === 'inference' || r.value === 'local_tools',
    );
    expect(profiles).toHaveLength(2);
    expect(profiles.some((r) => r.checked)).toBe(false);
    // Starting is refused until one is chosen.
    const start = Array.from(
      document.querySelectorAll<HTMLButtonElement>('.projects-launch button'),
    ).find((b) => text(b) === 'Start')!;
    expect(start.disabled).toBe(true);
    profiles.find((r) => r.value === 'local_tools')!.click();
    await settle();
    expect(start.disabled).toBe(false);
    start.click();
    await settle();
    expect(client.sent[0]!.command).toMatchObject({
      kind: 'launch',
      provider: 'codex',
      mode: 'managed',
      nativeProfile: null,
      managedSafetyProfile: 'local_tools',
    });
  });

  it('offers Claude Managed as unavailable, not as a way to run Claude with Codex', async () => {
    const client = readyClient();
    await opened(client);
    const provider = document.querySelector<HTMLSelectElement>(
      'select[aria-label="What to start"]',
    )!;
    provider.value = 'claude';
    provider.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    const managed = Array.from(
      document.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    ).find((r) => r.value === 'managed')!;
    expect(managed.disabled).toBe(true);
    expect(text(document.querySelector('.projects-launch'))).toContain(
      'not available for Claude yet',
    );
  });

  it('offers a shell and nothing else in Scratch', async () => {
    const client = readyClient();
    await shown(client);
    // Scratch is in view with no panes: add one, then start in it.
    client.state.workspace = workspace({
      projectId: null,
      id: SCRATCH,
      layout: { kind: 'leaf', id: LEAF_1 },
    });
    await click('Scratch');
    await click('Start…');
    const options = Array.from(
      document.querySelectorAll<HTMLOptionElement>(
        'select[aria-label="What to start"] option',
      ),
    ).map((o) => o.value);
    expect(options).toEqual(['shell']);
  });
});

describe('a launch of unknown outcome', () => {
  it('is shown as unresolved, with only choices that start nothing', async () => {
    const client = readyClient();
    client.state.workspace = workspace({
      unfinished: [
        {
          operationId: 'aaaaaaaa-0000-4000-8000-0000000000aa',
          leafId: LEAF_1,
          state: 'ambiguous',
          mode: 'managed',
        },
      ],
    });
    await shown(client);
    await click('Alpha');
    const body = text(document.querySelector('.projects-unfinished'));
    expect(body).toContain('unknown outcome');
    expect(body).toContain('Nothing was started again');
    expect(button('Start…')).toBeUndefined();
    // A Managed launch is not looked for in a terminal host.
    expect(button('Look for what it started')).toBeUndefined();
    await click('Nothing started');
    expect(client.sent[0]!.command).toMatchObject({
      kind: 'reconcileLaunch',
      resolution: { kind: 'absent' },
    });
    await click('Ask the Managed owner again');
    expect(client.sent[1]!.command).toMatchObject({
      resolution: { kind: 'retryManaged' },
    });
  });
});

describe('a Native launch of unknown outcome', () => {
  const OP = 'aaaaaaaa-0000-4000-8000-0000000000aa';
  const stuck = () => {
    const client = readyClient();
    client.state.workspace = workspace({
      unfinished: [
        { operationId: OP, leafId: LEAF_1, state: 'ambiguous', mode: 'native' },
      ],
    });
    return client;
  };

  it('can be resolved by picking what it started, and never offers to ask a Managed owner', async () => {
    const client = stuck();
    client.orphans = vi.fn(async () => [
      {
        terminalId: 'tttttttt-0000-4000-8000-000000000001',
        nativeSessionId: null,
        folder: 'repo',
      },
    ]);
    await shown(client);
    await click('Alpha');
    expect(button('Ask the Managed owner again')).toBeUndefined();
    await click('Look for what it started');
    expect(client.orphans).toHaveBeenCalledWith(OP);
    expect(client.execute).not.toHaveBeenCalled();
    expect(text(document.querySelector('.projects-unfinished'))).toContain(
      'repo',
    );
    await click('This one');
    expect(client.sent[0]!.command).toMatchObject({
      kind: 'reconcileLaunch',
      launchOperationId: OP,
      resolution: {
        kind: 'identified',
        terminalId: 'tttttttt-0000-4000-8000-000000000001',
        sessionId: null,
      },
    });
  });

  it('says so when nothing unshown is running, and Nothing started stays the way out', async () => {
    const client = stuck();
    await shown(client);
    await click('Alpha');
    await click('Look for what it started');
    expect(text(document.querySelector('.projects-unfinished'))).toContain(
      'Nothing is running that is not already shown',
    );
    await click('Nothing started');
    expect(client.sent[0]!.command).toMatchObject({
      resolution: { kind: 'absent' },
    });
  });
});

describe('a session in a pane', () => {
  const withSession = (over: Record<string, unknown> = {}) => {
    const client = readyClient();
    client.state.workspace = workspace({
      leaves: [{ id: LEAF_1, title: 'Pane', session: session(over) }],
    });
    return client;
  };

  it('draws the output it reads and starts read-only', async () => {
    const client = withSession();
    client.state.output.push({
      data: new TextEncoder().encode('hello'),
      next: 5,
      eof: false,
      truncated: false,
    });
    await shown(client);
    await click('Alpha');
    await settle();
    expect(renderers).toHaveLength(1);
    expect(new TextDecoder().decode(renderers[0]!.written[0]!)).toBe('hello');
    expect(renderers[0]!.inputEnabled).toBe(false);
    expect(text(document.querySelector('.projects-session'))).toContain(
      'Read-only view',
    );
    expect(client.claimInput).not.toHaveBeenCalled();
  });

  it('takes input only when asked, types through the desktop, and lets go when the window loses the keyboard', async () => {
    const client = withSession();
    await shown(client);
    await click('Alpha');
    await click('Take input');
    expect(client.claimInput).toHaveBeenCalledWith(ASSOCIATION_1);
    expect(renderers[0]!.inputEnabled).toBe(true);
    renderers[0]!.typing[0]!.text('ls\n');
    await settle();
    expect(client.writeInput).toHaveBeenCalledWith(
      ASSOCIATION_1,
      new TextEncoder().encode('ls\n'),
    );
    window.dispatchEvent(new Event('blur'));
    await settle();
    expect(client.releaseInput).toHaveBeenCalledWith(ASSOCIATION_1);
    expect(renderers[0]!.inputEnabled).toBe(false);
  });

  it('sends what is typed in the order it was typed, one call at a time', async () => {
    const client = withSession();
    const releases: (() => void)[] = [];
    client.writeInput = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          releases.push(resolve);
        }),
    );
    await shown(client);
    await click('Alpha');
    await click('Take input');
    renderers[0]!.typing[0]!.text('a');
    renderers[0]!.typing[0]!.text('b');
    renderers[0]!.typing[0]!.text('c');
    await settle();
    expect(client.writeInput).toHaveBeenCalledTimes(1);
    releases.shift()!();
    await settle();
    expect(client.writeInput).toHaveBeenCalledTimes(2);
    releases.shift()!();
    await settle();
    releases.shift()!();
    await settle();
    const sent = vi
      .mocked(client.writeInput)
      .mock.calls.map(([, data]) => new TextDecoder().decode(data));
    expect(sent).toEqual(['a', 'b', 'c']);
  });

  it('sends a long paste in pieces the host takes, in order, and stops at the first refusal', async () => {
    const client = withSession();
    await shown(client);
    await click('Alpha');
    await click('Take input');
    const paste = 'x'.repeat(4096 * 2 + 100);
    renderers[0]!.typing[0]!.text(paste);
    await settle();
    const sizes = vi
      .mocked(client.writeInput)
      .mock.calls.map(([, data]) => data.length);
    expect(sizes).toEqual([4096, 4096, 100]);
    // A refusal of one piece ends the paste: the rest is not sent after it.
    const refusing = withSession();
    refusing.writeInput = vi.fn(async () => {
      throw failure('owner_refused');
    });
    document.body.innerHTML = '';
    await shown(refusing);
    await click('Alpha');
    await click('Take input');
    renderers.at(-1)!.typing[0]!.text(paste);
    await settle();
    expect(refusing.writeInput).toHaveBeenCalledTimes(1);
  });

  it('wires the keyboard once when input is taken twice at the same moment', async () => {
    const client = withSession();
    await shown(client);
    await click('Alpha');
    const take = button('Take input')!;
    take.click();
    take.click();
    await settle();
    expect(client.claimInput).toHaveBeenCalledTimes(1);
    expect(renderers[0]!.typing).toHaveLength(1);
    renderers[0]!.typing[0]!.text('x');
    await settle();
    expect(client.writeInput).toHaveBeenCalledTimes(1);
  });

  it('gives input back and stops reading when the view is hidden or the pane goes away', async () => {
    const client = withSession();
    await shown(client);
    await click('Alpha');
    await click('Take input');
    expect(renderers[0]!.typing).toHaveLength(1);
    const reads = vi.mocked(client.readOutput).mock.calls.length;
    document.querySelector<HTMLButtonElement>('.test-toggle')!.click();
    await settle();
    expect(client.releaseInput).toHaveBeenCalledWith(ASSOCIATION_1);
    expect(renderers[0]!.typing).toHaveLength(0);
    document.querySelector<HTMLButtonElement>('.test-toggle')!.click();
    await settle();
    // Back on screen: a new pane, read-only again, and it asked for nothing in between.
    expect(renderers).toHaveLength(2);
    expect(renderers[1]!.inputEnabled).toBe(false);
    expect(vi.mocked(client.readOutput).mock.calls.length).toBeGreaterThan(
      reads,
    );
  });

  it('gives a claim back that arrives after the pane went away', async () => {
    const client = withSession();
    let grant: () => void = () => undefined;
    client.claimInput = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          grant = resolve;
        }),
    );
    await shown(client);
    await click('Alpha');
    button('Take input')!.click();
    await settle();
    document.querySelector<HTMLButtonElement>('.test-toggle')!.click();
    await settle();
    grant();
    await settle();
    expect(client.releaseInput).toHaveBeenCalledWith(ASSOCIATION_1);
    expect(renderers[0]!.typing).toHaveLength(0);
  });

  it('takes input at once for the session this window just started, and for no other', async () => {
    const client = readyClient();
    await shown(client);
    await click('Alpha');
    await click('Start…');
    client.state.receipt = {
      ...client.state.receipt,
      created: [{ role: 'association', id: ASSOCIATION_1 }],
    };
    // The refreshed workspace shows the new session.
    client.execute = vi.fn(async (envelope) => {
      client.sent.push(envelope);
      client.state.workspace = workspace({
        leaves: [{ id: LEAF_1, title: 'Pane', session: session() }],
      });
      return client.state.receipt;
    });
    await click('Start');
    await settle();
    expect(client.claimInput).toHaveBeenCalledWith(ASSOCIATION_1);
    expect(renderers[0]!.inputEnabled).toBe(true);
  });

  it('stays a mirror when another window holds input, with no way to type', async () => {
    const client = withSession({ input: 'other' });
    await shown(client);
    await click('Alpha');
    expect(button('Take input')).toBeUndefined();
    expect(text(document.querySelector('.projects-session'))).toContain(
      'Another window has input',
    );
    expect(renderers[0]!.inputEnabled).toBe(false);
  });

  it('stays read-only when taking input is refused', async () => {
    const client = withSession();
    client.state.claimError = failure('owner_refused', {
      field: 'input_claim',
    });
    await shown(client);
    await click('Alpha');
    await click('Take input');
    expect(renderers[0]!.inputEnabled).toBe(false);
    expect(text(document.querySelector('.projects-session'))).toContain(
      'Another window holds input',
    );
  });

  it('shows an ended or unreachable session without offering input', async () => {
    const client = withSession({
      availability: 'ended',
      reason: 'host_changed',
    });
    await shown(client);
    await click('Alpha');
    expect(button('Take input')).toBeUndefined();
    expect(text(document.querySelector('.projects-session'))).toContain(
      'ended (host changed)',
    );
  });

  it('detaches without closing anything', async () => {
    const client = withSession();
    await shown(client);
    await click('Alpha');
    await click('Detach');
    expect(client.sent[0]!.command).toMatchObject({
      kind: 'detach',
      workspaceId: WORKSPACE_A,
      leafId: LEAF_1,
      associationId: ASSOCIATION_1,
    });
  });

  it('shows a Managed session as facts with the requested and attached profile apart, and no terminal', async () => {
    const client = withSession({
      kind: 'managed',
      terminalId: null,
      protocol: 2,
      storeStreamId: 'bbbbbbbb-0000-4000-8000-000000000002',
      managedSessionId: 'cccccccc-0000-4000-8000-000000000001',
      requestedSafetyProfile: 'inference',
      effectiveSafetyProfile: 'local_tools',
      profileMismatch: true,
    });
    await shown(client);
    await click('Alpha');
    expect(renderers).toHaveLength(0);
    const facts = text(document.querySelector('.projects-facts'));
    expect(facts).toContain('Protocol 2');
    expect(facts).toContain('Requested profile inference');
    expect(facts).toContain('Attached profile local_tools');
    expect(text(document.querySelector('.projects-warn'))).toContain(
      'different profile',
    );
    expect(button('Take input')).toBeUndefined();
  });
});

describe('the sessions list', () => {
  it('shows history that no pane shows, and adopts an unassigned shell only by choice', async () => {
    const client = readyClient();
    client.state.unassigned = [
      session({
        id: 'aaaaaaaa-0000-4000-8000-0000000000b1',
        originProjectId: null,
        state: 'detached',
      }),
    ];
    await shown(client);
    // Scratch is in view by default.
    const row = document.querySelector('.projects-session-row')!;
    expect(text(row)).toContain('Shell');
    const choose = row.querySelector<HTMLSelectElement>('select')!;
    expect(client.execute).not.toHaveBeenCalled();
    choose.value = PROJECT_A;
    choose.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    // Choosing only asks: the desktop says where the shell really is, nothing moves until it is
    // confirmed, and Cancel moves nothing.
    expect(client.execute).not.toHaveBeenCalled();
    expect(client.adoptionPreview).toHaveBeenCalledWith(
      'aaaaaaaa-0000-4000-8000-0000000000b1',
      PROJECT_A,
    );
    const ask = text(document.querySelector('.projects-adopt'));
    expect(ask).toContain('/home/fixture/work/a/src');
    expect(ask).toContain('inside Alpha');
    expect(ask).toContain('Move it into Alpha');
    await click('Cancel');
    expect(client.execute).not.toHaveBeenCalled();
    choose.value = PROJECT_A;
    choose.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    await click('Move');
    expect(client.sent[0]!.command).toMatchObject({
      kind: 'adoptScratch',
      projectId: PROJECT_A,
      associationId: 'aaaaaaaa-0000-4000-8000-0000000000b1',
      // The binding the desktop named when it showed the directory.
      bindingId: BINDING_A,
    });
  });

  it('says so when the shell is outside the project folder, and asks nothing when the directory cannot be read', async () => {
    const client = readyClient();
    client.state.unassigned = [
      session({
        id: 'aaaaaaaa-0000-4000-8000-0000000000b1',
        originProjectId: null,
        state: 'detached',
      }),
    ];
    client.state.adoption = {
      bindingId: BINDING_A,
      cwd: '/tmp/elsewhere',
      inside: false,
    };
    await shown(client);
    const choose = document.querySelector<HTMLSelectElement>(
      '.projects-session-row select',
    )!;
    choose.value = PROJECT_A;
    choose.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(text(document.querySelector('.projects-adopt'))).toContain(
      'outside the folder of Alpha',
    );
    await click('Cancel');
    // The desktop could not read where the shell is: nothing is offered, and the reason shows.
    client.state.adoption = failure('owner_refused', { field: 'cwd' });
    choose.value = PROJECT_A;
    choose.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(document.querySelector('.projects-adopt')).toBeNull();
    expect(button('Move')).toBeUndefined();
    expect(text(document.querySelector('.projects-banner'))).toContain(
      'cannot be read, so nothing was moved',
    );
    expect(client.execute).not.toHaveBeenCalled();
  });

  it('offers to show a live session in an empty pane', async () => {
    const client = readyClient();
    client.state.sessions = [session({ state: 'detached' })];
    client.state.projects = [project()];
    await shown(client);
    await click('Alpha');
    expect(client.execute).not.toHaveBeenCalled();
    await click('Show in an empty pane');
    expect(client.sent[0]!.command).toMatchObject({
      kind: 'attach',
      leafId: LEAF_1,
      associationId: ASSOCIATION_1,
    });
  });
});

describe('long lists', () => {
  const many = (n: number) =>
    Array.from({ length: n }, (_, i) =>
      session({
        id: `aaaaaaaa-0000-4000-8000-${String(i + 1).padStart(12, '0')}`,
        originProjectId: null,
        state: 'detached',
        terminalId: `bbbbbbbb-0000-4000-8000-${String(i + 1).padStart(12, '0')}`,
      }),
    );

  it('draws the rows that can be seen, not all of a store at its ceiling', async () => {
    const client = readyClient();
    client.state.unassigned = many(500);
    await shown(client);
    const list = document.querySelector<HTMLElement>(
      '.projects-sessions .projects-virtual',
    )!;
    expect(list.dataset.rows).toBe('500');
    const drawn = document.querySelectorAll(
      '.projects-sessions .projects-session-row',
    ).length;
    expect(drawn).toBeGreaterThan(0);
    expect(drawn).toBeLessThan(40);
    // Scrolling draws the rows that come into view and lets go of the others.
    list.scrollTop = 44 * 300;
    list.dispatchEvent(new Event('scroll'));
    await settle();
    const rows = Array.from(
      document.querySelectorAll('.projects-sessions .projects-session-row'),
    );
    expect(rows.length).toBeLessThan(40);
    expect(text(rows[0]!)).toContain('bbbbbbbb');
    expect(rows.some((r) => r.getAttribute('data-row') === '301')).toBe(true);
  });

  it('draws a short list whole', async () => {
    const client = readyClient();
    client.state.unassigned = many(5);
    await shown(client);
    expect(document.querySelector('.projects-virtual')).toBeNull();
    expect(
      document.querySelectorAll('.projects-sessions .projects-session-row'),
    ).toHaveLength(5);
  });

  it('draws only the projects in view in the rail', async () => {
    const client = readyClient();
    client.state.projects = Array.from({ length: 200 }, (_, n) =>
      project({
        id: `cccccccc-0000-4000-8000-${String(n + 1).padStart(12, '0')}`,
        label: `Project ${n + 1}`,
      }),
    );
    await shown(client);
    const rail = document.querySelector<HTMLElement>(
      '.projects-rail .projects-virtual',
    )!;
    expect(rail.dataset.rows).toBe('200');
    expect(Number(rail.dataset.drawn)).toBeLessThan(40);
    // Scratch is always there, outside the long list.
    expect(text(document.querySelector('.projects-rail'))).toContain('Scratch');
  });
});

it('keeps a hidden view out of the way', async () => {
  const client = readyClient();
  component = mount(ProjectsHarness, {
    target: document.body,
    props: { client, visible: false },
  });
  await settle();
  const view = document.querySelector<HTMLElement>('.projects-feature')!;
  expect(view.hidden).toBe(true);
  expect(view.inert).toBe(true);
  expect(client.overview).not.toHaveBeenCalled();
});
