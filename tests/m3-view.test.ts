import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import M3Harness from './M3Harness.svelte';
import type { M3Client } from '../src/lib/desktop/m3-client';
import { parseM3State, type Ack } from '../src/lib/contracts/m3';
import inactive from './fixtures/m3/state-inactive.json';
import inference from './fixtures/m3/state-fixture-inference.json';
import held from './fixtures/m3/state-fixture-tools-held.json';
import ended from './fixtures/m3/state-fixture-tools-ended.json';
import refused from './fixtures/m3/state-fixture-refused.json';
import failed from './fixtures/m3/state-fixture-failed.json';

type Any = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any
const copy = <T>(v: T): T => JSON.parse(JSON.stringify(v));
const ok: Ack = { ok: true, error: null };
let component: ReturnType<typeof mount> | undefined;
const flush = async () => {
  for (let i = 0; i < 40; i++) await Promise.resolve();
  flushSync();
};
const text = (n: Element | null | undefined) =>
  n?.textContent?.replace(/\s+/g, ' ').trim() ?? '';
const q = (s: string) => document.querySelector(s);
const qa = (s: string) => [...document.querySelectorAll(s)];

function show(raw: unknown, extra: Partial<M3Client> = {}) {
  const client: M3Client = {
    state: vi.fn(async () => parseM3State(copy(raw))),
    fixtureStart: vi.fn(async () => ok),
    fixtureStop: vi.fn(async () => ok),
    fixturePrompt: vi.fn(async () => ok),
    fixtureInterrupt: vi.fn(async () => ok),
    fixtureDecide: vi.fn(async () => ok),
    ...extra,
  };
  component = mount(M3Harness, { target: document.body, props: { client } });
  return client;
}
beforeEach(() => {
  document.body.innerHTML = '';
});
afterEach(() => {
  if (component) unmount(component);
  component = undefined;
});

it('tells the inactive state: four profiles, Claude blocked with its reasons, no start action anywhere', async () => {
  show(inactive);
  await flush();
  const cards = qa('[data-testid^="profile-"]');
  expect(cards).toHaveLength(4);
  const claude = q('[data-testid="profile-claude_inference"]')!;
  expect(text(claude)).toContain('Blocked');
  expect(text(claude)).toContain('Claude Managed security and authentication');
  expect(text(claude)).toContain('only accepts Codex runtimes');
  expect(text(claude)).toContain('No authorized run against the real provider');
  expect(text(claude)).toContain('Real provider: not run');
  expect(text(claude)).toContain('Fixtures: passed');
  // No Start button exists for any profile, and no fixture console is present.
  expect(qa('button').filter((b) => /start/i.test(text(b)))).toHaveLength(0);
  expect(q('[data-testid="fixture-console"]')).toBeNull();
  expect(text(q('[data-testid="profile-codex_inference"]'))).toContain(
    'Unverified',
  );
});

it('states that a listed model is not an entitlement and that selection is off with its reason', async () => {
  show(inference);
  await flush();
  const facts = q('[data-testid="model-facts"]')!;
  expect(text(q('[data-testid="not-entitlement"]'))).toContain(
    'Offered is not entitled',
  );
  for (const h of ['Offered', 'Configured', 'Requested', 'Reported in use'])
    expect(text(facts)).toContain(h);
  const selection = q('[data-testid="model-selection"]')!;
  expect(selection.querySelectorAll('select:disabled')).toHaveLength(2);
  expect(text(selection)).toContain(
    'has not accepted'.replace(
      'has not accepted',
      'a decision the owner has not accepted',
    ),
  );
});

it('shows a reroute as a reroute and an effort as seen in the request, not proven', async () => {
  show(inference);
  await flush();
  const reroute = q('[data-testid="reroute"]')!;
  expect(reroute.getAttribute('data-state')).toBe('rerouted');
  expect(text(reroute)).toContain('Requested claude-sonnet-5-5');
  expect(text(reroute)).toContain('claude-haiku-5-5');
  const effort = q('[data-testid="effort-evidence"]')!;
  expect(effort.getAttribute('data-state')).toBe('not_requested');
});

it('shows unreported counters as "not reported" and never as zero', async () => {
  show(inference);
  await flush();
  const turns = qa('[data-testid="usage-turn"]');
  expect(turns).toHaveLength(2);
  expect(text(turns[0])).toContain('in not reported');
  expect(text(turns[0])).toContain('the provider reported nothing');
  expect(text(turns[0])).not.toMatch(/\bin 0\b/);
  expect(text(turns[1])).toContain('in 11');
  expect(text(turns[1])).toContain('Answered by claude-haiku-5-5');
  // A running total over turns that did not all report says so.
  expect(text(q('[data-testid="running-input"]'))).toContain('at least 11');
  expect(text(q('[data-testid="running-input"]'))).toContain(
    '1 turn did not report',
  );
});

it('keeps the SDK estimate out of sight, labelled not billing', async () => {
  show(inference);
  await flush();
  const est = q('[data-testid="estimate"]') as HTMLDetailsElement;
  expect(est.open).toBe(false);
  expect(text(est)).toContain('not billing');
  expect(text(est)).toContain('not your subscription');
});

it('shows a limit as an allowance, with reset time, never as money', async () => {
  show(inference);
  await flush();
  const limit = q('[data-testid="limit"]')!;
  expect(text(limit)).toContain('5-hour window');
  expect(text(limit)).toContain('72 % used');
  expect(text(limit)).toContain('An allowance, not a price');
  expect(text(limit)).not.toMatch(/\$/);
});

it('reports what the tap saw of the provider requests', async () => {
  show(inference);
  await flush();
  expect(text(q('[data-testid="tap-summary"]'))).toContain(
    'tools offered: none',
  );
});

it('offers the history only as off, with its reason and its limits', async () => {
  show(inactive);
  await flush();
  const h = q('[data-testid="history-settings"]')!;
  expect(
    (h.querySelector('input[type="checkbox"]') as HTMLInputElement).disabled,
  ).toBe(true);
  expect(text(h)).toContain('privacy and retention policy');
  expect(text(h)).toContain('10,000 records');
  expect(text(h)).toContain('no prompts, no answers, no reasoning');
  expect(
    qa('button')
      .filter((b) => /export|delete/i.test(text(b)))
      .every((b) => (b as HTMLButtonElement).disabled),
  ).toBe(true);
});

it('labels a fixture runtime as not the authority and starts nothing by itself', async () => {
  const c = show(refused);
  await flush();
  expect(text(q('[data-testid="fixture-label"]'))).toContain('FIXTURE HOST');
  expect(text(q('[data-testid="fixture-label"]'))).toContain(
    'no real provider is contacted',
  );
  expect(c.fixtureStart).not.toHaveBeenCalled();
  expect(text(q('[data-testid="fixture-failure"]'))).toContain(
    'refused and stopped',
  );
  expect(text(q('[data-testid="fixture-failure"]'))).toContain(
    'managed_policy_present',
  );
});

it('starts a fixture runtime only when asked, once per press', async () => {
  const c = show(failed);
  await flush();
  const start = qa('button').find(
    (b) => text(b) === 'Start Claude Local tools',
  ) as HTMLButtonElement;
  start.click();
  await flush();
  expect(c.fixtureStart).toHaveBeenCalledTimes(1);
  expect(c.fixtureStart).toHaveBeenCalledWith('claude_local_tools_v1');
});

it('shows the held command exactly and decides on it with the item it names', async () => {
  const c = show(held);
  await flush();
  const action = q('[data-testid="held-action"]')!;
  expect(text(q('[data-testid="held-command"]'))).toBe('cargo build --release');
  expect(text(action)).toContain('with no network');
  expect(text(action)).toContain('Waiting for your decision');
  const accept = action.querySelector(
    '[data-decision="accept"]',
  ) as HTMLButtonElement;
  accept.click();
  await flush();
  expect(c.fixtureDecide).toHaveBeenCalledWith('claude-act-1', 'accept');
  action.querySelector<HTMLButtonElement>('[data-decision="decline"]')!.click();
  await flush();
  expect(c.fixtureDecide).toHaveBeenLastCalledWith('claude-act-1', 'decline');
});

it('shows every way a call can end and offers a decision only on the pending one', async () => {
  show(ended);
  await flush();
  const actions = qa('[data-testid="held-action"]');
  const states = actions.map((a) => a.getAttribute('data-state'));
  expect(states).toEqual([
    'confirmed',
    'declined',
    'failed',
    'indeterminate',
    'pending',
  ]);
  const withButtons = actions.filter((a) => a.querySelector('[data-decision]'));
  expect(withButtons).toHaveLength(1);
  expect(text(actions[3])).toContain('will not be repeated');
  expect(text(actions[3])).toContain('check the workspace');
  expect(text(actions[2])).toContain('nothing changed');
  // The pending one is a file change: the whole diff is on screen.
  expect(text(q('[data-testid="held-diff"]'))).toContain('+line one');
  expect(text(q('[data-testid="held-diff"]'))).toContain('+line two');
});

it('makes a bidirectional override in a command visible instead of letting it hide', async () => {
  const doc = copy(held) as Any;
  doc.fixture.approvals[0].action.command = 'ls ‮; rm -rf ~';
  show(doc);
  await flush();
  expect(text(q('[data-testid="held-command"]'))).toContain('⟨U+202E⟩');
});

it('cannot accept an action it does not recognise, and says so', async () => {
  const doc = copy(held) as Any;
  doc.fixture.approvals[0].action = { kind: 'telepathy', do: 'it' };
  show(doc);
  await flush();
  const action = q('[data-testid="held-action"]')!;
  expect(action.querySelector('[data-decision="accept"]')).toBeNull();
  expect(action.querySelector('[data-decision="decline"]')).not.toBeNull();
  expect(text(action)).toContain('cannot be accepted');
});

it('sends a prompt once and blocks sending while a turn runs', async () => {
  const c = show(held);
  await flush();
  const send = q('[data-testid="send"]') as HTMLButtonElement;
  // The held fixture has a running turn: nothing can be sent, and the turn can be cancelled.
  expect(send.disabled).toBe(true);
  const cancel = q('[data-testid="interrupt"]') as HTMLButtonElement;
  expect(cancel.disabled).toBe(false);
  cancel.click();
  await flush();
  expect(c.fixtureInterrupt).toHaveBeenCalledTimes(1);
});

it('types and sends a prompt on a ready, idle runtime', async () => {
  const doc = copy(inference) as Any;
  doc.fixture.phase = 'ready';
  doc.fixture.turn = { id: 't', state: 'completed' };
  const c = show(doc);
  await flush();
  const box = q('[data-testid="prompt"]') as HTMLTextAreaElement;
  box.value = 'what changed?';
  box.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
  (q('[data-testid="send"]') as HTMLButtonElement).click();
  await flush();
  expect(c.fixturePrompt).toHaveBeenCalledWith('what changed?');
});

it('renders the conversation as text, not as markup', async () => {
  const doc = copy(inference) as Any;
  doc.fixture.conversation = [
    { role: 'assistant', text: '<img src=x onerror=alert(1)> **bold**' },
  ];
  show(doc);
  await flush();
  expect(q('[data-testid="transcript"] img')).toBeNull();
  expect(text(q('[data-testid="transcript"]'))).toContain(
    '<img src=x onerror=alert(1)> **bold**',
  );
});

it('says the data may have changed when a refresh failed, and keeps showing what it has', async () => {
  let fail = false;
  const c = show(inactive, {
    state: vi.fn(async () => {
      if (fail) throw new Error('down');
      return parseM3State(copy(inactive));
    }),
  });
  await flush();
  fail = true;
  qa('button')
    .find((b) => text(b) === 'Refresh')!
    .dispatchEvent(new MouseEvent('click', { bubbles: true }));
  await flush();
  expect(text(q('[data-testid="stale"]'))).toContain('may have changed');
  expect(q('[data-testid="profile-claude_inference"]')).not.toBeNull();
  expect(c.state).toHaveBeenCalledTimes(2);
});

it('reads nothing while hidden', async () => {
  const client = show(inactive);
  unmount(component!);
  component = mount(M3Harness, {
    target: document.body,
    props: { client: client, visible: false },
  });
  const before = (client.state as ReturnType<typeof vi.fn>).mock.calls.length;
  await flush();
  expect((client.state as ReturnType<typeof vi.fn>).mock.calls.length).toBe(
    before,
  );
});
