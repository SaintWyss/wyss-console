import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import main from '../src-tauri/capabilities/main.json';

describe('delegation desktop permissions', () => {
  it.each(['native_delegations', 'native_delegation_cancel'])(
    'registers and permits %s for the local window',
    (command) => {
      const build = readFileSync('src-tauri/build.rs', 'utf8');
      expect(build).toContain(`"${command}"`);
      expect(main.permissions).toContain(
        `allow-${command.replaceAll('_', '-')}`,
      );
    },
  );
});
