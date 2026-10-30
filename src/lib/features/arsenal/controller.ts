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
} from '../../contracts/arsenal';
import {
  arsenalClient,
  ArsenalCallError,
  type ArsenalClient,
} from '../../desktop/arsenal-client';
import { failureMessage, importMessage } from './messages';

export type ArsenalTab =
  | 'library'
  | 'discover'
  | 'composites'
  | 'presets'
  | 'selections'
  | 'terminals';

export interface Notice {
  tone: 'ok' | 'error';
  text: string;
}

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
  notice: Notice | null;
  presets: Preset[];
  selections: Selection[];
  preview: Preview | null;
  /** What started terminals carry, newest first. */
  bindings: Binding[];
}

/**
 * The one owner of Arsenal in the window. It reads the library while its view is shown and
 * changes only what the owner asks for: an import, a restore, a saved preset or selection. It
 * never sends a path, and nothing it holds starts a session.
 */
export class ArsenalController {
  private state: ArsenalState = {
    tab: 'library',
    phase: 'idle',
    library: [],
    opened: null,
    file: null,
    discover: null,
    discovering: false,
    chosen: null,
    chosenDetail: null,
    busy: null,
    notice: null,
    presets: [],
    selections: [],
    preview: null,
    bindings: [],
  };
  private visible = false;
  private disposed = false;

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
    if (visible) void this.refresh();
  }

  dispose() {
    this.disposed = true;
  }

  setTab(tab: ArsenalTab) {
    this.set({ tab, notice: null });
    if (tab === 'presets') void this.loadPresets();
    if (tab === 'selections') {
      void this.loadSelections();
      void this.loadPresets();
      // The composites a selection may keep come from the scan.
      if (this.state.discover === null) void this.discover();
    }
    if (tab === 'terminals') void this.loadBindings();
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
        notice: {
          tone: 'ok',
          text: `Saved selection ${input.name.trim()}. It is not applied.`,
        },
      });
      await this.loadSelections();
      return true;
    } catch (error) {
      this.set({
        busy: null,
        notice: { tone: 'error', text: failureMessage(error) },
      });
      return false;
    }
  }

  async deleteSelection(id: string) {
    try {
      await this.client.deleteSelection(id);
      this.set({
        notice: { tone: 'ok', text: 'Selection deleted.' },
        preview: null,
      });
      await this.loadSelections();
    } catch (error) {
      this.set({ notice: { tone: 'error', text: failureMessage(error) } });
    }
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
}

function failureOf(error: unknown): string {
  return error instanceof ArsenalCallError ? error.code : 'internal';
}
