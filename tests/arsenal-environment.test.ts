import { describe, expect, it } from 'vitest';
import {
  DEFAULT_SPEC,
  hookLabel,
  isDefault,
  parseOptions,
  parseSpec,
  toggle,
} from '../src/lib/features/arsenal/environment';

describe('arsenal launch environment', () => {
  it('treats the owner configuration as the default and any change as its own folder', () => {
    expect(isDefault(DEFAULT_SPEC)).toBe(true);
    expect(isDefault({ ...DEFAULT_SPEC, memory: false })).toBe(false);
    expect(isDefault({ ...DEFAULT_SPEC, mcp: [] })).toBe(false);
    expect(
      isDefault({ ...DEFAULT_SPEC, hooks_off: ['0123456789abcdef'] }),
    ).toBe(false);
  });

  it('keeps toggled lists sorted and unique', () => {
    expect(toggle(['b'], 'a', true)).toEqual(['a', 'b']);
    expect(toggle(['a', 'b'], 'a', true)).toEqual(['a', 'b']);
    expect(toggle(['a', 'b'], 'a', false)).toEqual(['b']);
  });

  it('names a hook by its event, matcher and script', () => {
    expect(
      hookLabel({
        key: 'k',
        event: 'PreToolUse',
        matcher: 'Bash',
        command: 'python3 /home/u/.claude/hooks/model-routing-gate.py',
      }),
    ).toBe('PreToolUse (Bash) · model-routing-gate.py');
    expect(
      hookLabel({
        key: 'k',
        event: 'SessionStart',
        matcher: '',
        command: 'rtk hook claude',
      }),
    ).toBe('SessionStart · rtk');
  });

  it('parses what the desktop answers and refuses a malformed reply', () => {
    expect(parseSpec({ ...DEFAULT_SPEC, mcp: ['context7'] }).mcp).toEqual([
      'context7',
    ]);
    expect(() => parseSpec({ ...DEFAULT_SPEC, memory: 'yes' })).toThrow();
    const options = parseOptions({
      hooks: [{ key: 'k', event: 'E', matcher: '', command: 'c' }],
      mcp: ['context7'],
      plugins: [],
      rules: ['ux.md'],
      instructions: true,
      token_stored: false,
    });
    expect(options.hooks).toHaveLength(1);
    expect(() => parseOptions({ hooks: [{ key: 1 }] })).toThrow();
  });
});
