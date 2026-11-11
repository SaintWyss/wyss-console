import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import { resetAppearance } from '../src/lib/features/terminal/appearance';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';

let renderer: TerminalRenderer;
let host: HTMLDivElement;
beforeEach(async () => {
  resetAppearance();
  vi.stubGlobal('matchMedia', () => ({
    matches: false,
    media: '',
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
  }));
  // jsdom has no rasterizer. Only text metrics are supplied; xterm itself parses
  // real byte/ANSI input and creates its actual DOM screen, without a parser mock.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
    () =>
      ({
        font: '',
        measureText: (text: string) => ({ width: text.length * 8 }),
        fillRect() {},
        getImageData: () => ({ data: new Uint8Array([0, 0, 0, 255]) }),
        createLinearGradient: () => ({ addColorStop() {} }),
      }) as unknown as CanvasRenderingContext2D,
  );
  document.documentElement.style.setProperty('--terminal-bg', '#20252c');
  document.documentElement.style.setProperty('--terminal-fg', '#e9edf3');
  host = document.createElement('div');
  document.body.append(host);
  const { createRenderer } =
    await import('../src/lib/features/terminal/renderer');
  renderer = createRenderer(host);
});
afterEach(() => {
  renderer?.dispose();
  resetAppearance();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it('opens the official renderer and preserves UTF-8 split across byte writes', async () => {
  expect(host.querySelector('.xterm')).not.toBeNull();
  await renderer.write(Uint8Array.of(0xe2));
  await renderer.write(Uint8Array.of(0x82, 0xac, 13, 10));
  await new Promise((resolve) => setTimeout(resolve, 30));
  expect(host.querySelector('.xterm-rows')?.textContent).toContain('€');
  expect(host.querySelector('.xterm-rows')?.textContent).not.toContain('�');
});
it('uses the real ANSI parser rather than displaying escape text', async () => {
  await renderer.write(new TextEncoder().encode('\x1b[31mRED\x1b[0m\r\n'));
  await new Promise((resolve) => setTimeout(resolve, 30));
  expect(host.querySelector('.xterm-rows')?.textContent).toContain('RED');
  expect(host.querySelector('.xterm-rows')?.textContent).not.toContain('[31m');
  const spans = Array.from(
    host.querySelectorAll<HTMLSpanElement>('.xterm-rows span'),
  );
  expect(
    spans.some(
      (span) =>
        span.textContent === 'RED' && span.classList.contains('xterm-fg-1'),
    ),
  ).toBe(true);
});
it('skips fit while the container cannot be measured', () => {
  expect(renderer.fit()).toBeUndefined();
});
it('disposes appearance listeners and cancels a pending byte write', async () => {
  const pending = renderer.write(Uint8Array.of(1));
  const refit = vi.fn();
  renderer.onMetricsChange!(refit);
  renderer.dispose();
  expect(await pending).toBe(false);
  expect(host.querySelector('.xterm')).toBeNull();
  const { currentAppearance, setAppearance } =
    await import('../src/lib/features/terminal/appearance');
  setAppearance({ ...currentAppearance(), fontSize: 18 });
  expect(refit).not.toHaveBeenCalled();
  renderer.dispose();
});

it('keeps hidden byte parsing and protocol responses alive while gating user keys/paste', async () => {
  const data = vi.fn();
  renderer.input(data, vi.fn());
  renderer.setInput(true);
  renderer.setActive(false);
  expect(host.inert).toBe(true);
  await renderer.write(new TextEncoder().encode('\x1b[6n'));
  expect(data).toHaveBeenCalledExactlyOnceWith('\x1b[1;1R');
  data.mockClear();
  const textarea = host.querySelector('textarea')!;
  textarea.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'a',
      code: 'KeyA',
      keyCode: 65,
      bubbles: true,
      cancelable: true,
    }),
  );
  const paste = new Event('paste', { bubbles: true, cancelable: true });
  textarea.dispatchEvent(paste);
  expect(paste.defaultPrevented).toBe(true);
  expect(data).not.toHaveBeenCalled();
  renderer.setActive(true);
  expect(host.inert).toBe(false);
  textarea.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'a',
      code: 'KeyA',
      keyCode: 65,
      bubbles: true,
      cancelable: true,
    }),
  );
  expect(data).toHaveBeenCalledWith('a');
});

// IBus under WebKitGTK reports a plain Space as keydown 229 ("Unidentified", not composing)
// followed by a cancelable beforeinput. xterm's own fallback for that path diffs the helper
// textarea, which is never cleared, and re-sent the word typed before the Space ("fg fg   fg").
const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
function imeKey(
  textarea: HTMLTextAreaElement,
  inputType: string,
  data: string | null,
  imeRewrites: string,
) {
  textarea.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'Unidentified',
      keyCode: 229,
      bubbles: true,
      cancelable: true,
    }),
  );
  const before = new InputEvent('beforeinput', {
    inputType,
    data,
    bubbles: true,
    cancelable: true,
    composed: true,
  });
  textarea.dispatchEvent(before);
  if (!before.defaultPrevented) {
    // What the IME does when nobody cancels it: it rewrites the whole word.
    textarea.value = imeRewrites;
    textarea.dispatchEvent(
      new InputEvent('input', {
        inputType,
        data,
        bubbles: true,
        composed: true,
      }),
    );
  }
}

it('sends a Space the IME reports as keydown 229 exactly once, without re-sending the word before it', async () => {
  const data = vi.fn();
  renderer.input(data, vi.fn());
  renderer.setInput(true);
  const textarea = host.querySelector('textarea')!;
  imeKey(textarea, 'insertText', ' ', 'fg ');
  await tick();
  imeKey(textarea, 'insertText', ' ', 'fg  ');
  await tick();
  expect(data.mock.calls.map(([text]) => text)).toEqual([' ', ' ']);
  expect(textarea.value).toBe('');
});

it('sends a character the IME reports as keydown 229 once, and a deletion as DEL', async () => {
  const data = vi.fn();
  renderer.input(data, vi.fn());
  renderer.setInput(true);
  const textarea = host.querySelector('textarea')!;
  imeKey(textarea, 'insertText', '2', 'fg2');
  imeKey(textarea, 'deleteContentBackward', null, 'fg');
  await tick();
  expect(data.mock.calls.map(([text]) => text)).toEqual(['2', '\x7f']);
});

it('leaves a real composition (dead keys) to xterm and sends the composed text once', async () => {
  const data = vi.fn();
  renderer.input(data, vi.fn());
  renderer.setInput(true);
  const textarea = host.querySelector('textarea')!;
  textarea.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'Dead',
      keyCode: 229,
      bubbles: true,
      cancelable: true,
    }),
  );
  textarea.dispatchEvent(
    new CompositionEvent('compositionstart', { bubbles: true }),
  );
  textarea.dispatchEvent(
    new CompositionEvent('compositionupdate', { data: '´', bubbles: true }),
  );
  textarea.dispatchEvent(
    new KeyboardEvent('keydown', {
      key: 'Unidentified',
      keyCode: 229,
      isComposing: true,
      bubbles: true,
      cancelable: true,
    }),
  );
  const during = new InputEvent('beforeinput', {
    inputType: 'insertCompositionText',
    data: 'é',
    isComposing: true,
    bubbles: true,
    cancelable: true,
    composed: true,
  });
  textarea.dispatchEvent(during);
  expect(during.defaultPrevented).toBe(false);
  textarea.value = 'é';
  textarea.dispatchEvent(
    new CompositionEvent('compositionupdate', { data: 'é', bubbles: true }),
  );
  await tick();
  textarea.dispatchEvent(
    new CompositionEvent('compositionend', { data: 'é', bubbles: true }),
  );
  await tick();
  expect(data.mock.calls.map(([text]) => text)).toEqual(['é']);
});

it('follows the appearance preferences, and says when a character changed size', async () => {
  const { setAppearance } =
    await import('../src/lib/features/terminal/appearance');
  const refit = vi.fn();
  const stop = renderer.onMetricsChange!(refit);
  const size = () =>
    host.querySelector<HTMLElement>('.xterm-char-measure-element')!.style
      .fontSize;
  expect(size()).toBe('16px');

  setAppearance({
    fontSize: 20,
    scrollback: 9000,
    cursorStyle: 'bar',
    cursorBlink: true,
    theme: 'dark',
  });
  expect(size()).toBe('20px');
  expect(refit).toHaveBeenCalledTimes(1);
  // A change that leaves the size of a character alone needs no new fit.
  setAppearance({
    fontSize: 20,
    scrollback: 3000,
    cursorStyle: 'underline',
    cursorBlink: false,
    theme: 'dark',
  });
  expect(refit).toHaveBeenCalledTimes(1);
  // The same appearance again wakes nobody.
  setAppearance({
    fontSize: 20,
    scrollback: 3000,
    cursorStyle: 'underline',
    cursorBlink: false,
    theme: 'dark',
  });
  expect(refit).toHaveBeenCalledTimes(1);

  stop();
  setAppearance({
    fontSize: 12,
    scrollback: 3000,
    cursorStyle: 'underline',
    cursorBlink: false,
    theme: 'dark',
  });
  expect(size()).toBe('12px');
  expect(refit).toHaveBeenCalledTimes(1);

  // A renderer that is gone no longer follows: nothing is kept alive for it.
  renderer.dispose();
  expect(() =>
    setAppearance({
      fontSize: 14,
      scrollback: 3000,
      cursorStyle: 'underline',
      cursorBlink: false,
      theme: 'dark',
    }),
  ).not.toThrow();
  resetAppearance();
});

it('starts a terminal with the appearance in force', async () => {
  renderer.dispose();
  const { resetAppearance, setAppearance } =
    await import('../src/lib/features/terminal/appearance');
  setAppearance({
    fontSize: 18,
    scrollback: 7000,
    cursorStyle: 'block',
    cursorBlink: false,
    theme: 'dark',
  });
  const { createRenderer } =
    await import('../src/lib/features/terminal/renderer');
  const second = document.createElement('div');
  document.body.append(second);
  renderer = createRenderer(second);
  expect(
    second.querySelector<HTMLElement>('.xterm-char-measure-element')!.style
      .fontSize,
  ).toBe('18px');
  resetAppearance();
});
it('searches the text on screen and in the scrollback, steps through matches, and clears', async () => {
  await renderer.write(
    new TextEncoder().encode('alpha beta\r\nBeta gamma\r\nbeta end\r\n'),
  );
  await new Promise((resolve) => setTimeout(resolve, 30));
  expect(renderer.search!('beta', { caseSensitive: false, step: 0 })).toEqual({
    total: 3,
    current: 1,
  });
  expect(
    renderer.search!('beta', { caseSensitive: false, step: 1 }).current,
  ).toBe(2);
  expect(
    renderer.search!('beta', { caseSensitive: false, step: -1 }).current,
  ).toBe(1);
  expect(renderer.search!('beta', { caseSensitive: true, step: 0 }).total).toBe(
    2,
  );
  expect(
    renderer.search!('nothing', { caseSensitive: false, step: 0 }),
  ).toEqual({
    total: 0,
    current: 0,
  });
  expect(renderer.search!('', { caseSensitive: false, step: 0 }).total).toBe(0);
  renderer.clearSearch!();
});
