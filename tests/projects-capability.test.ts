import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import main from '../src-tauri/capabilities/main.json';
import projects from '../src-tauri/capabilities/projects.json';
import config from '../src-tauri/tauri.conf.json';

const read = (path: string) =>
  readFileSync(join(__dirname, '..', 'src-tauri', path), 'utf8');
const permission = (command: string) => `allow-${command.replace(/_/g, '-')}`;

it('grants exactly the commands the extension routes, and only in the projects capability', () => {
  const source = read('src/projects/commands.rs');
  const block = /const COMMANDS: &\[&str\] = &\[([^\]]*)\];/.exec(source)![1]!;
  const routed = [...block.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]!);
  expect(routed.length).toBeGreaterThan(15);
  expect(projects.permissions).toEqual(routed.map(permission));
  for (const command of routed) {
    expect(main.permissions).not.toContain(permission(command));
    expect(read('build.rs')).toContain(`"${command}"`);
  }
});

it('lists the capability in the one configuration and gives other windows nothing else', () => {
  expect(config.app.security.capabilities).toContain('projects');
  expect(projects.identifier).toBe('projects');
  expect(projects.windows).toEqual(['main', 'w-*']);
  // The main window keeps what it had; a window the project workspace opens has this only.
  expect(main.windows).toEqual(['main']);
});

it('names no path, process, method or generic call in what it grants', () => {
  for (const name of projects.permissions)
    expect(name).not.toMatch(
      /path|exec|shell|process|method|invoke|raw|file|command/,
    );
});
