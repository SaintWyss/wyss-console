import { invoke } from '@tauri-apps/api/core';
import {
  parseApplied,
  parseChanges,
  parseConfigurationFailure,
  parseDone,
  parseHistory,
  parseInventoryReply,
  parsePlan,
  parseProject,
  parseRecords,
  type Applied,
  type Changes,
  type ConfigurationFailure,
  type History,
  type InventoryReply,
  type NewDocumentKind,
  type Operation,
  type ParkedRecord,
  type Plan,
} from '../contracts/configuration';

/**
 * The two reads of the agents' configuration the window can make. Neither names a path or
 * a file; both answer from what the desktop already holds, so asking often costs the
 * providers' files nothing.
 */
export class ConfigurationCallError extends Error {
  constructor(readonly code: ConfigurationFailure) {
    super(`configuration: ${code}`);
  }
}

async function call(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args);
  } catch (error) {
    throw new ConfigurationCallError(parseConfigurationFailure(error));
  }
}

export async function readConfigurationInventory(): Promise<InventoryReply> {
  const answer = await call('configuration_inventory', {});
  try {
    return parseInventoryReply(answer);
  } catch {
    // The desktop answered something that is not an inventory: nothing is believed.
    throw new ConfigurationCallError('internal');
  }
}

export async function readConfigurationChanges(
  since: number,
): Promise<Changes> {
  if (!Number.isSafeInteger(since) || since < 0)
    throw new ConfigurationCallError('internal');
  const answer = await call('configuration_changes', { since });
  try {
    return parseChanges(answer);
  } catch {
    throw new ConfigurationCallError('internal');
  }
}

// ---- changing (F2a) ----------------------------------------------------------------------
// Each call names what the desktop already holds: an item, a record, a plan or a change by
// its identifier, a source by its closed word. No call carries a path or the text of an
// entry, and every answer is checked before it is believed.

async function checked<T>(
  command: string,
  args: Record<string, unknown>,
  parse: (value: unknown) => T,
): Promise<T> {
  const answer = await call(command, args);
  try {
    return parse(answer);
  } catch {
    throw new ConfigurationCallError('internal');
  }
}

export function planChange(
  operation: Operation,
  target: string,
): Promise<Plan> {
  return checked(
    'configuration_plan',
    { request: { operation, target } },
    parsePlan,
  );
}
/**
 * Writing: the page names a kind of thing and where it goes, and nothing that runs. The
 * desktop opens a window of its own for the text, so no command is ever built from text
 * the page chose (DN-12).
 */
export function authorHook(
  event: string,
  matcher: string | null,
  timeout: number | null,
  fromTemplate = false,
): Promise<Plan> {
  return checked(
    'configuration_author_hook',
    { request: { event, matcher, timeout, from_template: fromTemplate } },
    parsePlan,
  );
}
/** A hook or an MCP server saved as a sealed template; its content never reaches the page. */
export function exportTemplate(item: string): Promise<boolean> {
  return checked(
    'configuration_template_export',
    { request: { item } },
    parseDone,
  );
}
export function exportDocument(item: string): Promise<boolean> {
  return checked(
    'configuration_document_export',
    { request: { item } },
    parseDone,
  );
}
export function importDocument(item: string): Promise<Plan> {
  return checked(
    'configuration_document_import',
    { request: { item } },
    parsePlan,
  );
}
export function editDocument(item: string): Promise<Plan> {
  return checked(
    'configuration_edit_document',
    { request: { item } },
    parsePlan,
  );
}
/** A new instruction file or skill where none exists. The desktop asks for name and text. */
export function createDocument(
  kind: NewDocumentKind,
  fromTemplate = false,
  scope: 'account' | 'project' = 'account',
): Promise<Plan> {
  return checked(
    'configuration_create_document',
    { request: { kind, from_template: fromTemplate, scope } },
    parsePlan,
  );
}
/** The selected project's folder name, or none. */
export function readProject(): Promise<string | null> {
  return checked('configuration_project', {}, parseProject);
}
/** Opens the desktop's own folder window (or, with `clear`, selects none). */
export function selectProject(clear: boolean): Promise<string | null> {
  return checked(
    'configuration_select_project',
    { request: { clear } },
    parseProject,
  );
}
export function editHook(item: string): Promise<Plan> {
  return checked('configuration_edit_hook', { request: { item } }, parsePlan);
}
export function authorMcp(
  name: string,
  kind: 'local' | 'remote',
  agent: 'opencode' | 'codex' = 'opencode',
  fromTemplate = false,
): Promise<Plan> {
  return checked(
    'configuration_author_mcp',
    { request: { name, kind, agent, from_template: fromTemplate } },
    parsePlan,
  );
}
export function editMcp(item: string): Promise<Plan> {
  return checked('configuration_edit_mcp', { request: { item } }, parsePlan);
}
export function applyPlan(plan: string): Promise<Applied> {
  return checked('configuration_apply', { request: { plan } }, parseApplied);
}
export function readHistory(
  cursor: string | null,
  limit = 20,
): Promise<History> {
  return checked(
    'configuration_history',
    { request: { cursor, limit } },
    parseHistory,
  );
}
export function planUndo(change: string): Promise<Plan> {
  return checked('configuration_undo', { request: { change } }, parsePlan);
}
export function readRecords(): Promise<ParkedRecord[]> {
  return checked('configuration_records', {}, parseRecords);
}
export function deleteRecord(record: string): Promise<boolean> {
  return checked(
    'configuration_record_delete',
    { request: { record } },
    parseDone,
  );
}
/** The desktop asks for a passphrase and a file in its own windows. */
export function exportRecord(record: string): Promise<boolean> {
  return checked('configuration_export', { request: { record } }, parseDone);
}
export function importRecord(): Promise<boolean> {
  return checked('configuration_import', {}, parseDone);
}
