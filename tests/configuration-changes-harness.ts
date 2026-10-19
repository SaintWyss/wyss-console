import {
  parseApplied,
  parseHistory,
  parsePlan,
  parseRecords,
  type Applied,
  type History,
  type Operation,
  type ParkedRecord,
  type Plan,
} from '../src/lib/contracts/configuration';
import type { ChangesClient } from '../src/lib/features/configuration/changes-controller';
import { ConfigurationCallError } from '../src/lib/desktop/configuration-client';
import changes from './fixtures/configuration-changes-v1.json';

/** The replies the desktop sends, as the shared fixture records them, already checked. */
export const fixture = {
  parkPlan: (): Plan => parsePlan(structuredClone(changes.plan_park_blocking)),
  allHooksPlan: (): Plan => parsePlan(structuredClone(changes.plan_all_hooks)),
  skillPlan: (): Plan => parsePlan(structuredClone(changes.plan_skill)),
  codexMcpPlan: (): Plan => parsePlan(structuredClone(changes.plan_codex_mcp)),
  codexSkillPlan: (): Plan =>
    parsePlan(structuredClone(changes.plan_codex_skill)),
  restorePlan: (): Plan => parsePlan(structuredClone(changes.plan_restore)),
  applied: (): Applied => parseApplied(structuredClone(changes.applied)),
  refused: (): Applied =>
    parseApplied(structuredClone(changes.applied_refused)),
  history: (): History => parseHistory(structuredClone(changes.history)),
  records: (): ParkedRecord[] => parseRecords(structuredClone(changes.records)),
};

/** A desktop that plans, applies and remembers, to the extent a test needs. */
export class FakeChanges {
  calls: string[] = [];
  plan: Plan = fixture.parkPlan();
  applied: Applied = fixture.applied();
  history: History = fixture.history();
  records: ParkedRecord[] = fixture.records();
  failNext:
    'entry_gone' | 'key_store_unavailable' | 'cancelled' | 'internal' | null =
    null;
  /** Held applies: each waits for `release`. */
  hold: (() => void)[] | null = null;
  exported = true;

  private check() {
    if (this.failNext) {
      const code = this.failNext;
      this.failNext = null;
      throw new ConfigurationCallError(code);
    }
  }
  release() {
    this.hold?.shift()?.();
  }
  client(): ChangesClient {
    return {
      plan: async (operation: Operation, target: string) => {
        this.calls.push(`plan:${operation}:${target}`);
        this.check();
        return this.plan;
      },
      apply: async (plan: string) => {
        this.calls.push(`apply:${plan}`);
        if (this.hold) await new Promise<void>((r) => this.hold!.push(r));
        this.check();
        return this.applied;
      },
      history: async (cursor: string | null) => {
        this.calls.push(`history:${cursor}`);
        this.check();
        return this.history;
      },
      undo: async (change: string) => {
        this.calls.push(`undo:${change}`);
        this.check();
        return this.plan;
      },
      authorHook: async (
        event: string,
        matcher: string | null,
        timeout: number | null,
      ) => {
        this.calls.push(`author_hook:${event}:${matcher}:${timeout}`);
        this.check();
        return this.plan;
      },
      exportDocument: async (item: string) => {
        this.calls.push(`export_document:${item}`);
        this.check();
        return true;
      },
      importDocument: async (item: string) => {
        this.calls.push(`import_document:${item}`);
        this.check();
        return this.plan;
      },
      editDocument: async (item: string) => {
        this.calls.push(`edit_document:${item}`);
        this.check();
        return this.plan;
      },
      editHook: async (item: string) => {
        this.calls.push(`edit_hook:${item}`);
        this.check();
        return this.plan;
      },
      authorMcp: async (
        name: string,
        kind: 'local' | 'remote',
        agent: 'opencode' | 'codex' = 'opencode',
      ) => {
        this.calls.push(`author_mcp:${name}:${kind}:${agent}`);
        this.check();
        return this.plan;
      },
      editMcp: async (item: string) => {
        this.calls.push(`edit_mcp:${item}`);
        this.check();
        return this.plan;
      },
      records: async () => {
        this.calls.push('records');
        this.check();
        return this.records;
      },
      deleteRecord: async (record: string) => {
        this.calls.push(`delete:${record}`);
        this.check();
        return true;
      },
      exportRecord: async (record: string) => {
        this.calls.push(`export:${record}`);
        this.check();
        return this.exported;
      },
      importRecord: async () => {
        this.calls.push('import');
        this.check();
        return true;
      },
    };
  }
}
