import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import main from '../src-tauri/capabilities/main.json';
import f8 from '../src-tauri/capabilities/f8.json';
import projects from '../src-tauri/capabilities/projects.json';
import m3 from '../src-tauri/capabilities/m3.json';
import config from '../src-tauri/tauri.conf.json';

const read = (path: string) =>
  readFileSync(join(__dirname, '..', 'src-tauri', path), 'utf8');

it('registers, declares and grants the orchestration command consistently', () => {
  const source = read('src/orchestration_commands.rs');
  expect(source).toMatch(/const COMMANDS: &\[&str\] = &\["orchestration_call"\]/);
  expect(source).toMatch(/generate_handler!\[orchestration_call\]/);
  expect(source).toMatch(/request: DaemonRequest/);
  const declared = [...read('build.rs').matchAll(/"orchestration_call"/g)];
  expect(declared).toHaveLength(1);
  expect(
    main.permissions.filter((p) => p === 'allow-orchestration-call'),
  ).toHaveLength(1);
});

it('grants the command only through the main capability among all configured capabilities', () => {
  const capabilities = [main, f8, projects, m3];
  expect(config.app.security.capabilities).toEqual(
    capabilities.map((capability) => capability.identifier),
  );
  expect(main.windows).toEqual(['main']);
  expect(projects.windows).toEqual(['main', 'w-*']);
  for (const capability of capabilities.filter((item) => item !== main)) {
    expect(capability.permissions).not.toContain('allow-orchestration-call');
  }
});
