import type {
  ArsenalProvider,
  Binding,
  CandidateDetail,
  Detail,
  DiscoverReply,
  FileText,
  Item,
  Preset,
  Preview,
  LibraryItem,
  Selection,
  SyncStatus,
} from '../../contracts/arsenal';
import {
  arsenalClient,
  ArsenalCallError,
  type ArsenalClient,
} from '../../desktop/arsenal-client';
import { msg } from '../../i18n/msg';
import { failureMessage, importMessage } from './messages';
import { planKey } from './presets';

/** The sentence a refused group move shows on its row. */
const GROUP_NOT_SAVED = msg('Could not be saved. Nothing changed.');

export type ArsenalTab =
  'library' | 'discover' | 'composites' | 'presets' | 'activity';

export interface Notice {
  tone: 'ok' | 'error';
  text: string;
}

/**
 * What the desktop would resolve a draft to, asked for without saving it. `preview` is `null`
 * while the answer is pending, and when the check failed (`failed`).
 */
export interface DraftPlan {
  key: string;
  preview: Preview | null;
  failed: boolean;
}

/** One bulk import run: what each entry did, and what stayed out and why. */
export interface BulkRun {
  total: number;
  done: number;
  created: number;
  newRevision: number;
  unchanged: number;
  failures: { name: string; text: string }[];
  /** Why the run stopped early, when the library itself refused writes. */
  stopped: string | null;
  finished: boolean;
}

/** Codes that say the library cannot take writes at all: the run stops instead of failing item by item. */
const LIBRARY_STOPS = new Set([
  'storage_unavailable',
  'storage_corrupt',
  'storage_unsupported',
]);

export interface ArsenalState {
  tab: ArsenalTab;
  /** `unavailable`: the library could not be opened at all. `failed`: a read did not come back whole. */
  phase: 'idle' | 'loading' | 'ready' | 'unavailable' | 'failed';
  library: LibraryItem[];
  opened: Detail | null;
  file: FileText | null;
  discover: DiscoverReply | null;
  discovering: boolean;
  chosen: string | null;
  /** The files and exclusions of the chosen capability, read on selection. */
  chosenDetail: CandidateDetail | null;
  busy: 'import' | 'restore' | 'save' | null;
  /** The bulk run in progress or the last one finished, shown until the next run starts. */
  bulk: BulkRun | null;
  notice: Notice | null;
  presets: Preset[];
  selections: Selection[];
  preview: Preview | null;
  /** What started terminals carry, newest first. */
  bindings: Binding[];
  /** Synchronization with the owner's other computers; `null` until it was read once. */
  sync: SyncStatus | null;
  /** The owner's group corrections, by capability identity. Kept on this computer. */
  groups: Record<string, string>;
  /** The capability whose group move is being saved, if any. */
  movingGroup: string | null;
  /** A group move that was refused, shown on that capability's row. */
  groupFailure: { identity: string; text: string } | null;
  /** The check of the draft preset; `null` when there is nothing to check. */
  plan: DraftPlan | null;
}

/**
 * The one owner of Arsenal in the window. It reads the library while its view is shown and
 * changes only what the owner asks for: an import, a restore, a saved preset or selection. It
 * never sends a path, and nothing it holds starts a session.
 */
export class ArsenalController {
  private state: ArsenalState = {
    tab: 'presets',
    phase: 'idle',
    library: [],
    opened: null,
    file: null,
    discover: null,
    discovering: false,
    chosen: null,
    chosenDetail: null,
    busy: null,
    bulk: null,
    notice: null,
    presets: [],
    selections: [],
    preview: null,
    bindings: [],
    sync: null,
    groups: {},
    movingGroup: null,
    groupFailure: null,
    plan: null,
  };
  private visible = false;
  private disposed = false;
  /** The key of the draft whose answer is still wanted: an answer for any other key is dropped. */
  private planRequest: string | null = null;

  constructor(
    private readonly publish: (state: ArsenalState) => void,
    private readonly client: ArsenalClient = arsenalClient,
  ) {}

  snapshot(): ArsenalState {
    return { ...this.state };
  }

  private set(change: Partial<ArsenalState>) {
    this.state = { ...this.state, ...change };
    if (!this.disposed) this.publish(this.snapshot());
  }

  /** The library is read when its view is shown, and again each time it is shown. */
  setVisible(visible: boolean) {
    if (this.disposed || visible === this.visible) return;
    this.visible = visible;
    if (visible) {
      void this.refresh();
      if (this.state.tab === 'presets') void this.loadPresets();
      // The scan says which providers are installed and which composites exist for presets.
      if (this.state.discover === null) void this.discover();
    }
    this.watchSync(visible);
  }

  dispose() {
    this.disposed = true;
    this.watchSync(false);
  }

  private syncTimer: ReturnType<typeof setInterval> | null = null;
  private syncGeneration: number | null = null;

  /** Synchronization's status is read while the view is shown, and only then. */
  private watchSync(on: boolean) {
    if (this.syncTimer) clearInterval(this.syncTimer);
    this.syncTimer = null;
    if (!on || this.disposed) return;
    void this.readSync();
    this.syncTimer = setInterval(() => void this.readSync(), 4000);
  }

  /** Reads the status; when synchronization changed the library since the last read, reads that too. */
  async readSync() {
    try {
      const sync = await this.client.syncStatus();
      if (this.disposed) return;
      const changed =
        this.syncGeneration !== null && sync.generation !== this.syncGeneration;
      this.syncGeneration = sync.generation;
      this.set({ sync });
      if (changed) {
        void this.refresh();
        void this.loadPresets();
        void this.loadSelections();
      }
    } catch {
      // A desktop that does not synchronize answers nothing; the status stays as it was.
    }
  }

  /** Asks for a run now. Its result shows on the next read. */
  async syncNow() {
    try {
      this.set({ sync: await this.client.syncNow() });
    } catch {
      return;
    }
    setTimeout(() => void this.readSync(), 2500);
  }

  setTab(tab: ArsenalTab) {
    this.set({ tab, notice: null });
    if (tab === 'presets') void this.loadPresets();
    if (tab === 'activity') void this.loadBindings();
    if (
      (tab === 'discover' || tab === 'composites') &&
      this.state.discover === null
    )
      void this.discover();
  }

  clearNotice() {
    this.set({ notice: null });
  }

  /** Reads the library. Shown content stays on screen while it reads. */
  async refresh() {
    this.set({
      phase: this.state.library.length ? this.state.phase : 'loading',
    });
    try {
      const library = await this.client.capabilities();
      this.set({ library, phase: 'ready' });
      if (this.state.opened) {
        const stillThere = library.some(
          (item) => item.id === this.state.opened?.summary.id,
        );
        if (!stillThere) this.set({ opened: null, file: null });
      }
    } catch (error) {
      this.set({
        phase:
          failureOf(error) === 'storage_unavailable' ? 'unavailable' : 'failed',
      });
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  async open(capability: string, revision: number | null = null) {
    try {
      const opened = await this.client.capability(capability, revision);
      this.set({ opened, file: null, notice: null });
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  async showFile(path: string) {
    const opened = this.state.opened;
    if (!opened) return;
    try {
      const file = await this.client.file(
        opened.summary.id,
        opened.revision,
        path,
      );
      this.set({ file });
    } catch (error) {
      this.set({
        file: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
    }
  }

  async restore(revision: number) {
    const opened = this.state.opened;
    if (!opened) return;
    this.set({ busy: 'restore', notice: null });
    try {
      const restored = await this.client.restore(opened.summary.id, revision);
      this.set({
        opened: restored,
        file: null,
        busy: null,
        notice: {
          tone: 'ok',
          text: `Revision ${revision} is current again. No new revision was made.`,
        },
      });
      void this.refresh();
    } catch (error) {
      this.set({
        busy: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
    }
  }

  async discover() {
    this.set({ discovering: true, notice: null });
    try {
      const discover = await this.client.discover();
      this.set({
        discover,
        discovering: false,
        chosen: null,
        chosenDetail: null,
      });
    } catch (error) {
      this.set({
        discovering: false,
        notice: { tone: 'error', text: failureMessage(error) },
      });
    }
  }

  /** Chooses a discovered capability and reads its files from the last scan. */
  async choose(id: string | null) {
    this.set({ chosen: id, chosenDetail: null, notice: null });
    if (id === null) return;
    try {
      const chosenDetail = await this.client.candidateDetail(id);
      if (this.state.chosen === id) this.set({ chosenDetail });
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  async importCandidate(id: string) {
    const candidate = this.state.discover?.candidates.find(
      (entry) => entry.id === id,
    );
    this.set({ busy: 'import', notice: null });
    try {
      // The request names the entry the last scan listed. One it did not list is refused, not guessed.
      if (!candidate) {
        throw new ArsenalCallError({ code: 'not_found', reason: null });
      }
      const imported = await this.client.importSkill({
        id,
        kind: candidate.kind,
        source: candidate.source,
        name: candidate.name,
      });
      // The scan and the library are read first: those reads clear the notice, so it comes last.
      await this.discover();
      await this.refresh();
      this.set({
        busy: null,
        notice: {
          tone: 'ok',
          text: importMessage(candidate?.name ?? 'capability', imported),
        },
      });
    } catch (error) {
      this.set({
        busy: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
    }
  }

  /**
   * Imports every eligible entry of the last scan, one request each. The scan is not read again
   * until the run ends. A failed entry is recorded and the run goes on; a library that cannot take
   * writes stops it, since every later entry would fail the same way.
   */
  async importEligible() {
    if (this.state.busy !== null) return;
    const eligible = (this.state.discover?.candidates ?? []).filter(
      (candidate) =>
        candidate.importable &&
        (candidate.status === 'not_imported' ||
          candidate.status === 'source_changed'),
    );
    if (eligible.length === 0) {
      this.set({
        notice: {
          tone: 'ok',
          text: 'Nothing to import: every eligible skill is already in the library.',
        },
      });
      return;
    }
    const run: BulkRun = {
      total: eligible.length,
      done: 0,
      created: 0,
      newRevision: 0,
      unchanged: 0,
      failures: [],
      stopped: null,
      finished: false,
    };
    this.set({ busy: 'import', notice: null, bulk: { ...run } });
    for (const candidate of eligible) {
      try {
        const imported = await this.client.importSkill({
          id: candidate.id,
          kind: candidate.kind,
          source: candidate.source,
          name: candidate.name,
        });
        if (imported.outcome === 'created') run.created += 1;
        else if (imported.outcome === 'new_revision') run.newRevision += 1;
        else run.unchanged += 1;
      } catch (error) {
        if (LIBRARY_STOPS.has(failureOf(error))) {
          run.stopped = failureMessage(error);
          break;
        }
        run.failures.push({
          name: candidate.name,
          text: failureMessage(error),
        });
      }
      run.done += 1;
      this.set({ bulk: { ...run, failures: [...run.failures] } });
    }
    run.finished = true;
    this.set({ bulk: { ...run, failures: [...run.failures] } });
    await this.discover();
    await this.refresh();
    const failed = run.failures.length + (run.stopped ? 1 : 0);
    this.set({
      busy: null,
      notice: {
        tone: failed > 0 ? 'error' : 'ok',
        text: bulkSummary(run),
      },
    });
  }

  async loadPresets() {
    try {
      this.set({ presets: await this.client.presets() });
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  async savePreset(input: {
    id: string | null;
    name: string;
    description: string;
    items: Item[];
  }) {
    this.set({ busy: 'save', notice: null });
    try {
      await this.client.savePreset(input);
      this.set({
        busy: null,
        notice: { tone: 'ok', text: `Saved preset ${input.name.trim()}.` },
      });
      await this.loadPresets();
      return true;
    } catch (error) {
      this.set({
        busy: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
      return false;
    }
  }

  async deletePreset(id: string) {
    try {
      await this.client.deletePreset(id);
      this.set({ notice: { tone: 'ok', text: 'Preset deleted.' } });
      await this.loadPresets();
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  async loadSelections() {
    try {
      this.set({ selections: await this.client.selections() });
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  async saveSelection(input: {
    id: string | null;
    name: string;
    provider: ArsenalProvider;
    items: Item[];
    composites: string[];
  }) {
    this.set({ busy: 'save', notice: null });
    try {
      const saved = await this.client.saveSelection(input);
      // A preview of this selection shows what was saved before: it is dropped, not left to mislead.
      const preview =
        this.state.preview?.selection.id === saved.id
          ? null
          : this.state.preview;
      this.set({
        busy: null,
        preview,
        notice: { tone: 'ok', text: msg('Preset saved') },
      });
      await this.loadSelections();
      return saved;
    } catch (error) {
      this.set({
        busy: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
      return null;
    }
  }

  /** Deletes a preset. Answers whether it was deleted; a refused delete leaves the list as it was. */
  async deleteSelection(id: string): Promise<boolean> {
    try {
      await this.client.deleteSelection(id);
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
      return false;
    }
    this.set({
      notice: { tone: 'ok', text: 'Selection deleted.' },
      preview: null,
    });
    await this.loadSelections();
    return true;
  }

  async loadPreview(id: string) {
    try {
      this.set({ preview: await this.client.preview(id), notice: null });
    } catch (error) {
      this.set({
        preview: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
    }
  }

  async loadBindings() {
    try {
      this.set({ bindings: await this.client.bindings() });
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  /** Reads the owner's group corrections. They are kept on this computer and never synchronized. */
  async loadGroups() {
    try {
      this.set({ groups: await this.client.groups() });
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
  }

  /**
   * Moves a capability to a group, or back to its automatic group (`null`). Its row shows
   * "Saving…" meanwhile. A refused move leaves the corrections as they were and says so on the row.
   */
  async moveGroup(identity: string, group: string | null) {
    this.set({ movingGroup: identity, groupFailure: null });
    try {
      await this.client.moveGroup(identity, group);
    } catch {
      this.set({
        movingGroup: null,
        groupFailure: { identity, text: GROUP_NOT_SAVED },
      });
      return;
    }
    this.set({ movingGroup: null });
    await this.loadGroups();
  }

  /**
   * Asks the desktop what a draft would resolve to, without saving it. Only the answer to the
   * latest key is kept. A draft with nothing chosen is not checked. `force` asks again for a key
   * that already has an answer or failed (the "Try again" action).
   */
  async checkDraft(
    provider: ArsenalProvider,
    items: Item[],
    composites: string[],
    force = false,
  ) {
    if (items.length === 0) {
      this.planRequest = null;
      this.set({ plan: null });
      return;
    }
    const key = planKey(provider, items, composites);
    if (!force && this.planRequest === key && this.state.plan !== null) return;
    this.planRequest = key;
    this.set({ plan: { key, preview: null, failed: false } });
    try {
      const preview = await this.client.planPreset(provider, items, composites);
      if (this.planRequest === key)
        this.set({ plan: { key, preview, failed: false } });
    } catch {
      if (this.planRequest === key)
        this.set({ plan: { key, preview: null, failed: true } });
    }
  }
}

function failureOf(error: unknown): string {
  return error instanceof ArsenalCallError ? error.code : 'internal';
}

/** The result a run leaves on screen: counts first, then what did not import and why. */
export function bulkSummary(run: BulkRun): string {
  const parts = [
    `${run.created} imported`,
    `${run.newRevision} updated to a new revision`,
    `${run.unchanged} already in the library, unchanged`,
  ];
  if (run.failures.length > 0) parts.push(`${run.failures.length} failed`);
  const text = `Bulk import: ${parts.join(', ')}.`;
  return run.stopped ? `${text} Stopped: ${run.stopped}` : text;
}
