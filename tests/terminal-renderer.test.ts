import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import type { TerminalRenderer } from '../src/lib/features/terminal/renderer';

let renderer: TerminalRenderer;
let host: HTMLDivElement;
let remove: ReturnType<typeof vi.fn>;
beforeEach(async () => {
  remove = vi.fn();
  vi.stubGlobal('matchMedia', () => ({
    addEventListener: vi.fn(),
    removeEventListener: remove,
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
    spans.some((span) => span.textContent === 'RED' && span.style.color),
  ).toBe(true);
});
it('skips fit while the container cannot be measured', () => {
  expect(renderer.fit()).toBeUndefined();
});
it('disposes DOM and media listeners and cancels a pending byte write', async () => {
  const pending = renderer.write(Uint8Array.of(1));
  renderer.dispose();
  expect(await pending).toBe(false);
  expect(host.querySelector('.xterm')).toBeNull();
  expect(remove).toHaveBeenCalledTimes(1);
  renderer.dispose();
  expect(remove).toHaveBeenCalledTimes(1);
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
