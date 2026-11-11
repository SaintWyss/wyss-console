/**
 * What the desktop says about a project's folder (Project Insight): counts, technologies,
 * architecture, types and README. Names are relative to the folder; no path ever comes back.
 * Every answer is checked against this shape and nothing that does not fit is believed.
 */
import { bool, list, oneOf, rec, str, strOrNull } from './projects';

export const TECHNOLOGY_CATEGORIES = [
  'language',
  'framework',
  'library',
  'tooling',
  'testing',
  'database',
  'infrastructure',
  'ci',
] as const;
export type TechnologyCategory = (typeof TECHNOLOGY_CATEGORIES)[number];
export const TYPE_KINDS = [
  'class',
  'interface',
  'struct',
  'enum',
  'trait',
] as const;
export type TypeKind = (typeof TYPE_KINDS)[number];

export interface Language {
  name: string;
  files: number;
  bytes: number;
}
export interface Technology {
  name: string;
  category: TechnologyCategory;
  version: string | null;
  source: string;
}
export interface Pattern {
  name: string;
  description: string;
  /** 0 to 100. */
  confidence: number;
  evidence: string[];
}
export interface Entry {
  name: string;
  dir: boolean;
  files: number;
}
export interface Script {
  source: string;
  name: string;
  command: string;
}
export interface Readme {
  name: string;
  content: string;
  truncated: boolean;
}
export interface TypeDecl {
  name: string;
  kind: TypeKind;
  file: string;
  language: string;
  extends: string[];
  implements: string[];
  fields: string[];
  methods: string[];
}
export interface Insight {
  name: string;
  files: number;
  folders: number;
  bytes: number;
  /** The walk stopped at a bound; the counts are a lower limit. */
  truncated: boolean;
  languages: Language[];
  technologies: Technology[];
  architecture: Pattern[];
  structure: Entry[];
  manifests: string[];
  scripts: Script[];
  git: boolean;
  license: string | null;
  readme: Readme | null;
  types: TypeDecl[];
}

function count(value: unknown, what: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    throw new Error(`projects: ${what}`);
  return value;
}
function names(value: unknown, what: string): string[] {
  return list(value, what, (v) => str(v, what));
}

function parseLanguage(value: unknown): Language {
  const v = rec(value, 'language');
  return {
    name: str(v.name, 'language.name', 128),
    files: count(v.files, 'language.files'),
    bytes: count(v.bytes, 'language.bytes'),
  };
}
function parseTechnology(value: unknown): Technology {
  const v = rec(value, 'technology');
  return {
    name: str(v.name, 'technology.name', 128),
    category: oneOf(v.category, TECHNOLOGY_CATEGORIES, 'technology.category'),
    version: strOrNull(v.version ?? null, 'technology.version'),
    source: str(v.source, 'technology.source', 512),
  };
}
function parsePattern(value: unknown): Pattern {
  const v = rec(value, 'pattern');
  const confidence = count(v.confidence, 'pattern.confidence');
  if (confidence > 100) throw new Error('projects: pattern.confidence');
  return {
    name: str(v.name, 'pattern.name', 128),
    description: str(v.description, 'pattern.description', 512),
    confidence,
    evidence: names(v.evidence, 'pattern.evidence'),
  };
}
function parseEntry(value: unknown): Entry {
  const v = rec(value, 'entry');
  return {
    name: str(v.name, 'entry.name', 512),
    dir: bool(v.dir, 'entry.dir'),
    files: count(v.files, 'entry.files'),
  };
}
function parseScript(value: unknown): Script {
  const v = rec(value, 'script');
  return {
    source: str(v.source, 'script.source', 512),
    name: str(v.name, 'script.name', 256),
    command: str(v.command, 'script.command', 4096),
  };
}
function parseReadme(value: unknown): Readme | null {
  if (value === null) return null;
  const v = rec(value, 'readme');
  return {
    name: str(v.name, 'readme.name', 512),
    content: str(v.content, 'readme.content', 512 * 1024),
    truncated: bool(v.truncated, 'readme.truncated'),
  };
}
function parseType(value: unknown): TypeDecl {
  const v = rec(value, 'type');
  return {
    name: str(v.name, 'type.name', 256),
    kind: oneOf(v.kind, TYPE_KINDS, 'type.kind'),
    file: str(v.file, 'type.file', 512),
    language: str(v.language, 'type.language', 64),
    extends: names(v.extends, 'type.extends'),
    implements: names(v.implements, 'type.implements'),
    fields: names(v.fields, 'type.fields'),
    methods: names(v.methods, 'type.methods'),
  };
}

/** Checks the answer of `projects_insight`; throws `projects: <field>` when it does not fit. */
export function parseInsight(value: unknown): Insight {
  const v = rec(value, 'insight');
  return {
    name: str(v.name, 'name', 512),
    files: count(v.files, 'files'),
    folders: count(v.folders, 'folders'),
    bytes: count(v.bytes, 'bytes'),
    truncated: bool(v.truncated, 'truncated'),
    languages: list(v.languages, 'languages', parseLanguage),
    technologies: list(v.technologies, 'technologies', parseTechnology),
    architecture: list(v.architecture, 'architecture', parsePattern),
    structure: list(v.structure, 'structure', parseEntry),
    manifests: names(v.manifests, 'manifests'),
    scripts: list(v.scripts, 'scripts', parseScript),
    git: bool(v.git, 'git'),
    license: strOrNull(v.license ?? null, 'license'),
    readme: parseReadme(v.readme ?? null),
    types: list(v.types, 'types', parseType),
  };
}

/** Checks the answer of `projects_readme_save`: the file name that was written. */
export function parseReadmeName(value: unknown): string {
  return str(value, 'readme_name', 512);
}
