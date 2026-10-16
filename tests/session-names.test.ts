import { expect, it } from 'vitest';
import {
  folderName,
  localTime,
  sessionLabel,
  sessionNames,
  shortId,
  type NameSource,
} from '../src/lib/features/sessions/present';

const id = (n: number) =>
  `${n.toString(16).padStart(8, '0')}-0000-4000-8000-000000000000`;
const at = (minute: number, second = 0) =>
  `2026-10-16T10:${String(minute).padStart(2, '0')}:${String(second).padStart(2, '0')}Z`;
const source = (
  n: number,
  workspace: string | null,
  created?: string | null,
): NameSource => ({ id: id(n), workspace, created_at: created });
const nameOf = (names: Map<string, string>, n: number) => names.get(id(n));

it('names a session for its folder, or for its identity when it has none', () => {
  expect(folderName('/home/dev/rag-corp')).toBe('rag-corp');
  expect(folderName('/home/dev/rag-corp/')).toBe('rag-corp');
  expect(folderName('/')).toBeNull();
  expect(folderName(null)).toBeNull();
  expect(shortId(id(0x1a2b3c4d))).toBe('1a2b3c4d');
  expect(sessionLabel(source(0x33333333, null))).toBe('Session 33333333');
  expect(sessionLabel(source(0x22222222, '/'))).toBe('Session 22222222');
});

it('adds nothing to a name that is already unique', () => {
  const names = sessionNames([
    source(1, '/home/dev/alpha', at(1)),
    source(2, '/home/dev/beta', at(2)),
    source(0x33333333, null, at(3)),
  ]);
  expect(nameOf(names, 1)).toBe('alpha');
  expect(nameOf(names, 2)).toBe('beta');
  expect(nameOf(names, 0x33333333)).toBe('Session 33333333');
});

it('tells sessions of one folder apart by when they were created', () => {
  const names = sessionNames([
    source(1, '/home/dev/app', at(1)),
    source(2, '/home/dev/app', at(30)),
    source(3, '/home/dev/other', at(5)),
  ]);
  expect(nameOf(names, 1)).toBe(`app · ${localTime(at(1))}`);
  expect(nameOf(names, 2)).toBe(`app · ${localTime(at(30))}`);
  expect(nameOf(names, 3)).toBe('other');
});

it('falls back to the short identity when the creation time does not tell them apart', () => {
  // Same minute: the written time is the same, so it cannot be what tells them apart.
  const sameMinute = sessionNames([
    source(0xaaaa0001, '/home/dev/app', at(1, 5)),
    source(0xbbbb0002, '/home/dev/app', at(1, 40)),
  ]);
  expect(nameOf(sameMinute, 0xaaaa0001)).toBe('app · aaaa0001');
  expect(nameOf(sameMinute, 0xbbbb0002)).toBe('app · bbbb0002');
  // No time known for one of them: none is used for either.
  const unknown = sessionNames([
    source(0xaaaa0001, '/home/dev/app', at(1)),
    source(0xbbbb0002, '/home/dev/app', null),
    source(0xcccc0003, '/home/dev/app'),
  ]);
  expect([...unknown.values()]).toEqual([
    'app · aaaa0001',
    'app · bbbb0002',
    'app · cccc0003',
  ]);
});

it('tells folders that end alike apart by their parents, only as deep as it takes', () => {
  const shallow = sessionNames([
    source(1, '/home/a/app'),
    source(2, '/home/b/app'),
  ]);
  expect(nameOf(shallow, 1)).toBe('a/app');
  expect(nameOf(shallow, 2)).toBe('b/app');
  const deep = sessionNames([
    source(1, '/work/x/lib/app'),
    source(2, '/work/y/lib/app'),
    source(3, '/work/z/other'),
  ]);
  expect(nameOf(deep, 1)).toBe('x/lib/app');
  expect(nameOf(deep, 2)).toBe('y/lib/app');
  expect(nameOf(deep, 3)).toBe('other');
});

it('combines the two: folders told apart, and several sessions in one of them', () => {
  const names = sessionNames([
    source(1, '/home/a/app', at(1)),
    source(2, '/home/a/app', at(20)),
    source(3, '/home/b/app', at(5)),
  ]);
  expect(nameOf(names, 1)).toBe(`a/app · ${localTime(at(1))}`);
  expect(nameOf(names, 2)).toBe(`a/app · ${localTime(at(20))}`);
  expect(nameOf(names, 3)).toBe('b/app');
});

it('does not depend on the order it is given, and ignores a session listed twice', () => {
  const list = [
    source(1, '/home/a/app', at(1)),
    source(2, '/home/a/app', at(20)),
    source(3, '/home/b/app', at(5)),
    source(4, '/home/dev/solo', at(6)),
  ];
  const forward = sessionNames(list);
  const backward = sessionNames([...list].reverse());
  const twice = sessionNames([...list, list[0]]);
  for (const entry of list) {
    expect(backward.get(entry.id)).toBe(forward.get(entry.id));
    expect(twice.get(entry.id)).toBe(forward.get(entry.id));
  }
  expect(twice.size).toBe(list.length);
});

it('never gives two sessions the same name, whatever the set', () => {
  // A small, seeded generator: the same sets every run.
  let seed = 0x2f6e2b1;
  const next = (bound: number) => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed % bound;
  };
  const folders = [
    '/h/app',
    '/h/lib/app',
    '/x/lib/app',
    '/h/web',
    '/y/web',
    '/',
    null,
  ];
  const times = [at(1), at(1, 30), at(2), at(3), null];
  for (let round = 0; round < 300; round++) {
    const count = 1 + next(9);
    const list: NameSource[] = Array.from({ length: count }, (_, n) => ({
      id: id(0x10000000 * (1 + (n % 15)) + n),
      workspace: folders[next(folders.length)],
      created_at: times[next(times.length)],
    }));
    const names = sessionNames(list);
    const written = [...names.values()];
    expect(new Set(written).size, JSON.stringify(list)).toBe(written.length);
    expect(written).toHaveLength(list.length);
    for (const name of written) expect(name.trim()).not.toBe('');
  }
});
