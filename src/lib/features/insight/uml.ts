/**
 * The class diagram of a project: the types its source declares, the relations between them
 * (extends, implements) and the text exports (Mermaid, PlantUML). Pure functions, no DOM.
 *
 * Relations point from a type (`from`) to the type it extends or implements (`to`); both are
 * indices into the array they were computed from.
 */
import type { TypeDecl } from '../../contracts/insight';

export interface Relation {
  from: number;
  to: number;
  kind: 'extends' | 'implements';
}

export interface LayoutBox {
  index: number;
  name: string;
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface Layout {
  boxes: LayoutBox[];
  width: number;
  height: number;
}

export interface UmlFilter {
  /** A folder of the project (`src/domain`); empty for the whole project. */
  folder: string;
  /** Matches the type name or its file, ignoring case. */
  text: string;
  /** Only types with a relation. Defaults to on when the project declares more than 60 types. */
  onlyRelated?: boolean;
}

export const RELATED_BY_DEFAULT_ABOVE = 60;

const CHAR_W = 7;
const BOX_PAD = 16;
const LINE_H = 16;
const HEAD_H = 24;
const GAP_X = 32;
const GAP_Y = 56;
const MARGIN = 24;

/** Where `to` points in `types`: the same file first, then the first type with that name. */
function resolve(
  types: TypeDecl[],
  name: string,
  file: string,
  self: number,
): number {
  let fallback = -1;
  for (let j = 0; j < types.length; j++) {
    if (j === self || types[j].name !== name) continue;
    if (types[j].file === file) return j;
    if (fallback === -1) fallback = j;
  }
  return fallback;
}

/** The extends and implements relations whose target is declared in `types`. */
export function relations(types: TypeDecl[]): Relation[] {
  const out: Relation[] = [];
  types.forEach((t, from) => {
    for (const kind of ['extends', 'implements'] as const) {
      for (const name of t[kind]) {
        const to = resolve(types, name, t.file, from);
        if (to !== -1) out.push({ from, to, kind });
      }
    }
  });
  return out;
}

/** Fields first, then methods: the members a box shows, in the order a reader expects. */
function members(t: TypeDecl): string[] {
  return [...t.fields, ...t.methods];
}

/** The header line of a box: the stereotype (for non-classes) and the name. */
export function headerText(t: TypeDecl): string {
  return t.kind === 'class' ? t.name : `«${t.kind}» ${t.name}`;
}

/** Unique diagram identifiers for the types, in order (`Name`, `Name_2`, ...). */
export function nodeIds(types: TypeDecl[]): string[] {
  const used = new Set<string>();
  return types.map((t) => {
    const cleaned = t.name.replace(/[^A-Za-z0-9_]/g, '_');
    const base = /^[A-Za-z_]/.test(cleaned) ? cleaned : `T_${cleaned}`;
    let id = base;
    for (let n = 2; used.has(id); n++) id = `${base}_${n}`;
    used.add(id);
    return id;
  });
}

/**
 * Keeps a member line inside the exporter's syntax: no braces or quotes, and Mermaid writes
 * generics with `~` where the rest of the text uses angle brackets.
 */
function memberText(text: string, mermaid: boolean): string {
  const generic = mermaid ? text.replace(/[<>]/g, '~') : text;
  return generic.replace(/[{}"]/g, '');
}

const MERMAID_STEREOTYPE: Record<TypeDecl['kind'], string | null> = {
  class: null,
  interface: 'interface',
  struct: 'struct',
  enum: 'enumeration',
  trait: 'interface',
};

export function toMermaid(types: TypeDecl[]): string {
  const ids = nodeIds(types);
  const lines = ['classDiagram'];
  types.forEach((t, i) => {
    lines.push(`  class ${ids[i]} {`);
    const stereotype = MERMAID_STEREOTYPE[t.kind];
    if (stereotype) lines.push(`    <<${stereotype}>>`);
    for (const m of members(t)) lines.push(`    +${memberText(m, true)}`);
    lines.push('  }');
  });
  for (const r of relations(types)) {
    const arrow = r.kind === 'extends' ? '<|--' : '<|..';
    lines.push(`  ${ids[r.to]} ${arrow} ${ids[r.from]}`);
  }
  return `${lines.join('\n')}\n`;
}

const PLANTUML_KEYWORD: Record<TypeDecl['kind'], string> = {
  class: 'class',
  interface: 'interface',
  struct: 'class',
  enum: 'enum',
  trait: 'interface',
};

export function toPlantUml(types: TypeDecl[]): string {
  const ids = nodeIds(types);
  const lines = ['@startuml'];
  types.forEach((t, i) => {
    const stereotype =
      t.kind === 'struct' || t.kind === 'trait' ? ` <<${t.kind}>>` : '';
    const head = `${PLANTUML_KEYWORD[t.kind]} ${ids[i]}${stereotype}`;
    const body = members(t);
    if (body.length === 0) {
      lines.push(head);
      return;
    }
    lines.push(`${head} {`);
    for (const m of body) lines.push(`  ${memberText(m, false)}`);
    lines.push('}');
  });
  for (const r of relations(types)) {
    const arrow = r.kind === 'extends' ? '--|>' : '..|>';
    lines.push(`${ids[r.from]} ${arrow} ${ids[r.to]}`);
  }
  lines.push('@enduml');
  return `${lines.join('\n')}\n`;
}

/** The types the diagram shows: by folder, by text, and (for large projects) only the linked ones. */
export function filterTypes(types: TypeDecl[], filter: UmlFilter): TypeDecl[] {
  const folder = filter.folder.trim().replace(/\/+$/, '');
  const text = filter.text.trim().toLowerCase();
  const shown = types.filter(
    (t) =>
      (!folder || t.file === folder || t.file.startsWith(`${folder}/`)) &&
      (!text ||
        t.name.toLowerCase().includes(text) ||
        t.file.toLowerCase().includes(text)),
  );
  const onlyRelated =
    filter.onlyRelated ?? types.length > RELATED_BY_DEFAULT_ABOVE;
  if (!onlyRelated) return shown;
  const linked = new Set<number>();
  for (const r of relations(shown)) {
    linked.add(r.from);
    linked.add(r.to);
  }
  return shown.filter((_, i) => linked.has(i));
}

/** The level of each type: 0 for a root, otherwise one below its deepest parent. */
function levels(count: number, rels: Relation[]): number[] {
  const parents: number[][] = Array.from({ length: count }, () => []);
  for (const r of rels) parents[r.from].push(r.to);
  const memo = new Array<number | undefined>(count);
  const onPath = new Array<boolean>(count).fill(false);
  const levelOf = (i: number): number => {
    const known = memo[i];
    if (known !== undefined) return known;
    if (onPath[i]) return 0; // a cycle in the declarations: cut it here
    onPath[i] = true;
    let level = 0;
    for (const p of parents[i]) level = Math.max(level, levelOf(p) + 1);
    onPath[i] = false;
    memo[i] = level;
    return level;
  };
  return Array.from({ length: count }, (_, i) => levelOf(i));
}

/** Rows by level, parents above their children; each box sized by its longest line. */
export function layout(types: TypeDecl[], rels: Relation[]): Layout {
  const rows: number[][] = [];
  levels(types.length, rels).forEach((level, i) => {
    (rows[level] ??= []).push(i);
  });
  const boxes: LayoutBox[] = [];
  let y = MARGIN;
  let width = 0;
  for (const row of rows) {
    let x = MARGIN;
    let rowHeight = 0;
    for (const i of row ?? []) {
      const t = types[i];
      const lines = members(t);
      const head = headerText(t).length;
      const longest = Math.max(head, ...lines.map((m) => m.length));
      const w = longest * CHAR_W + BOX_PAD;
      const h = HEAD_H + LINE_H * lines.length;
      boxes.push({ index: i, name: t.name, x, y, w, h });
      x += w + GAP_X;
      rowHeight = Math.max(rowHeight, h);
    }
    width = Math.max(width, x - GAP_X + MARGIN);
    y += rowHeight + GAP_Y;
  }
  boxes.sort((a, b) => a.index - b.index);
  return {
    boxes,
    width: width || 2 * MARGIN,
    height: rows.length ? y - GAP_Y + MARGIN : 2 * MARGIN,
  };
}
