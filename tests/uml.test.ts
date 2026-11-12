import { describe, expect, it } from 'vitest';
import type { TypeDecl } from '../src/lib/contracts/insight';
import {
  filterTypes,
  layout,
  nodeIds,
  relations,
  toMermaid,
  toPlantUml,
} from '../src/lib/features/insight/uml';

function decl(over: Partial<TypeDecl> & { name: string }): TypeDecl {
  return {
    kind: 'class',
    file: 'src/app.ts',
    language: 'TypeScript',
    extends: [],
    implements: [],
    fields: [],
    methods: [],
    ...over,
  };
}

const types: TypeDecl[] = [
  decl({ name: 'Base', file: 'src/base.ts', methods: ['run()'] }),
  decl({
    name: 'User',
    file: 'src/user.ts',
    extends: ['Base'],
    implements: ['Named'],
    fields: ['name: string', 'items: Vec<u8>'],
    methods: ['greet(): string'],
  }),
  decl({
    name: 'Named',
    kind: 'interface',
    file: 'src/named.ts',
    methods: ['name()'],
  }),
  decl({ name: 'Color', kind: 'enum', file: 'src/color.ts', fields: ['Red'] }),
];

describe('relations point from a type to what it extends or implements', () => {
  it('resolves targets declared in the list and skips the rest', () => {
    const rels = relations(types);
    expect(rels).toEqual([
      { from: 1, to: 0, kind: 'extends' },
      { from: 1, to: 2, kind: 'implements' },
    ]);
  });

  it('prefers the target in the same file when names repeat', () => {
    const twice = [
      decl({ name: 'Base', file: 'a.ts' }),
      decl({ name: 'Base', file: 'b.ts' }),
      decl({ name: 'Child', file: 'b.ts', extends: ['Base'] }),
    ];
    expect(relations(twice)).toEqual([{ from: 2, to: 1, kind: 'extends' }]);
  });
});

describe('the Mermaid and PlantUML exports are valid diagrams', () => {
  it('Mermaid: classes with stereotypes, members and relations', () => {
    const mmd = toMermaid(types);
    expect(mmd.startsWith('classDiagram\n')).toBe(true);
    expect(mmd).toContain('  class User {');
    expect(mmd).toContain('    +name: string');
    expect(mmd).toContain('    +items: Vec~u8~');
    expect(mmd).toContain('  class Named {\n    <<interface>>');
    expect(mmd).toContain('  class Color {\n    <<enumeration>>');
    expect(mmd).toContain('  Base <|-- User');
    expect(mmd).toContain('  Named <|.. User');
  });

  it('PlantUML: blocks, relations and the start and end markers', () => {
    const puml = toPlantUml(types);
    expect(puml.startsWith('@startuml\n')).toBe(true);
    expect(puml.trimEnd().endsWith('@enduml')).toBe(true);
    expect(puml).toContain('class User {\n  name: string');
    expect(puml).toContain('interface Named {');
    expect(puml).toContain('enum Color {');
    expect(puml).toContain('User --|> Base');
    expect(puml).toContain('User ..|> Named');
  });

  it('a name that is not an identifier is cleaned, and repeats get a suffix', () => {
    const ids = nodeIds([
      decl({ name: 'Vec<T>' }),
      decl({ name: 'Vec_T_' }),
      decl({ name: '2fa' }),
    ]);
    expect(new Set(ids).size).toBe(3);
    expect(ids[0]).toBe('Vec_T_');
    expect(ids[2]).toBe('T_2fa');
  });

  it('a member with braces or quotes cannot close the block', () => {
    const mmd = toMermaid([decl({ name: 'X', methods: ['run{}"() '] })]);
    expect(mmd).toContain('    +run() ');
    expect(mmd).not.toMatch(/[{}"]\s*\)/);
  });
});

describe('layout puts parents above their children', () => {
  it('rows by level, every box inside the reported size', () => {
    const rels = relations(types);
    const l = layout(types, rels);
    const byName = Object.fromEntries(l.boxes.map((b) => [b.name, b]));
    expect(l.boxes).toHaveLength(types.length);
    expect(byName.Base.y).toBeLessThan(byName.User.y);
    expect(byName.Named.y).toBeLessThan(byName.User.y);
    expect(byName.Color.y).toBe(byName.Base.y);
    for (const b of l.boxes) {
      expect(b.x + b.w).toBeLessThanOrEqual(l.width);
      expect(b.y + b.h).toBeLessThanOrEqual(l.height);
    }
  });

  it('a cycle in the declarations still lays out', () => {
    const loop = [
      decl({ name: 'A', extends: ['B'] }),
      decl({ name: 'B', extends: ['A'] }),
    ];
    const l = layout(loop, relations(loop));
    expect(l.boxes).toHaveLength(2);
  });
});

describe('filtering keeps the diagram readable', () => {
  it('filters by folder and by text', () => {
    expect(
      filterTypes(types, { folder: 'src', text: '', onlyRelated: false }),
    ).toHaveLength(4);
    expect(
      filterTypes(types, {
        folder: 'src/base.ts',
        text: '',
        onlyRelated: false,
      }),
    ).toHaveLength(1);
    expect(
      filterTypes(types, { folder: '', text: 'named', onlyRelated: false }),
    ).toHaveLength(1);
  });

  it('above 60 types, only the types with a relation are shown by default', () => {
    const many = [
      ...types,
      ...Array.from({ length: 60 }, (_, i) => decl({ name: `Lone${i}` })),
    ];
    const shown = filterTypes(many, { folder: '', text: '' });
    expect(shown.map((t) => t.name).sort()).toEqual(['Base', 'Named', 'User']);
  });
});
