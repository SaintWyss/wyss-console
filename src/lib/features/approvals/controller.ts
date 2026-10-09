import type {
  ApprovalDetail,
  ApprovalList,
  ApprovalSummary,
  CommandView,
  DecisionReceipt,
  FileChangeKind,
  FileView,
} from '../../contracts/approvals';
import type { ManagedApprovalId } from '../../contracts/identities';
import type { SessionFailure } from '../../contracts/sessions';
import {
  approveApproval,
  denyApproval,
  readApproval,
  readApprovals,
} from '../../desktop/approvals-client';
import { SessionCallError } from '../../desktop/sessions-client';
import type { AuthorityObservation } from '../../shell/authority';

export type ApprovalsLink = 'connecting' | 'offline' | 'connected';
/** What the user can ask for one approval. Nothing broader exists. */
export type DecisionKind = 'approve' | 'deny';

/** One file of a change set, as far as its diff was read. */
export interface FileCard {
  index: number;
  path: string;
  kind: FileChangeKind;
  move_path: string | null;
  /** Size of the whole diff, in bytes. */
  bytes: number;
  /** The diff from its beginning, as far as it was read. Plain text. */
  text: string;
  /** Where the next page starts. Null when the whole diff was read. */
  next: number | null;
  reading: boolean;
  failure: SessionFailure | null;
}
export interface DecisionView {
  kind: DecisionKind;
  status: 'pending' | 'failed';
  failure?: SessionFailure;
  /** The approval was read again after the decision failed: what is shown is current. */
  reread: boolean;
}
/** One pending approval with what was read of its action. */
export interface ApprovalCard {
  approval: ApprovalSummary;
  workspace: string | null;
  command: CommandView | null;
  /** How many files the change set has. */
  files: number | null;
  /** The files read so far, in order. */
  shown: FileCard[];
  /** More files are being read. */
  listing: boolean;
  decision: DecisionView | null;
}
export interface ApprovalsState {
  link: ApprovalsLink;
  reading: boolean;
  /** Why the last read failed, while there is nothing newer to show instead. */
  readFailure: SessionFailure | null;
  /** Null before the first read on this connection. */
  pending: ApprovalCard[] | null;
  /** Ended, newest first. */
  recent: ApprovalSummary[];
  partial: boolean;
}

export interface ApprovalsClient {
  list(): Promise<ApprovalList>;
  read(
    approval: ManagedApprovalId,
    file: number | null,
    offset: number | null,
  ): Promise<ApprovalDetail>;
  approve(
    intent: string,
    approval: ManagedApprovalId,
    revision: number,
  ): Promise<DecisionReceipt>;
  deny(
    intent: string,
    approval: ManagedApprovalId,
    revision: number,
  ): Promise<DecisionReceipt>;
}
const desktop: ApprovalsClient = {
  list: readApprovals,
  read: readApproval,
  approve: approveApproval,
  deny: denyApproval,
};

/** Files of a change set read without being asked for. The rest are read on request. */
export const FILES_AT_ONCE = 8;
/** Past an approval's deadline, how long until what became of it is read. */
export const EXPIRY_GRACE_MS = 400;

interface Card extends ApprovalCard {
  /** The decision that was asked for, kept whole: a retry sends exactly this. */
  intent: { id: string; kind: DecisionKind; revision: number } | null;
}

function sequence(text: string | null | undefined): bigint {
  return text ? BigInt(text) : 0n;
}
function failureOf(error: unknown): SessionFailure {
  return error instanceof SessionCallError
    ? error.failure
    : { code: 'internal', retry: false };
}
function fileCard(file: FileView): FileCard {
  return {
    index: file.index,
    path: file.path,
    kind: file.kind,
    move_path: file.move_path,
    bytes: file.bytes,
    text: file.chunk,
    next: file.next,
    reading: false,
    failure: null,
  };
}

/**
 * Presentation state of approvals, for the Approvals view and for the card shown under
 * a turn. It decides nothing: it reads what the authority holds, sends one decision the
 * user asked for, and reads again.
 *
 * It lives for the application's lifetime and has one reader. A list is read when the
 * authority's durable head moves, and only while somebody has a use for it: the
 * Approvals view is shown, or a turn is being followed. Nothing is read otherwise, and
 * nothing is polled: the one timer it keeps fires once, at the deadline of the approval
 * that expires first, to read what became of it.
 *
 * A decision is the user's one action. Its identity is made once, and only that identity
 * is sent again. While a decision is unsettled no other is sent for that approval, and
 * nothing here ever answers an approval the user did not answer.
 */
export class ApprovalsController {
  private link: ApprovalsLink = 'connecting';
  private head: string | null = null;
  private restarts = 0;
  private visible = false;
  private watching = false;
  /** Head of the list that is shown; undefined before one was read on this connection. */
  private shownHead: string | null | undefined = undefined;
  private triedHead: string | null = null;
  private triedRestarts = 0;
  private reading = false;
  private wanted = false;
  private readFailure: SessionFailure | null = null;
  private cards: Card[] = [];
  private recent: ApprovalSummary[] = [];
  private partial = false;
  /** Lists issued so far. An answer to anything but the latest is discarded. */
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  /** The approval a turn named that the last list did not have. */
  private triedFor: ManagedApprovalId | null = null;
  private disposed = false;

  constructor(
    private readonly update: (state: ApprovalsState) => void,
    private readonly client: ApprovalsClient = desktop,
    private readonly newIntent: () => string = () => crypto.randomUUID(),
  ) {}

  state(): ApprovalsState {
    return {
      link: this.link,
      reading: this.reading,
      readFailure: this.readFailure,
      pending:
        this.shownHead === undefined
          ? null
          : this.cards.map(({ intent, ...card }) => {
              void intent;
              return {
                ...card,
                shown: card.shown.map((file) => ({ ...file })),
                decision: card.decision && { ...card.decision },
              };
            }),
      recent: this.recent,
      partial: this.partial,
    };
  }
  private publish() {
    if (!this.disposed) this.update(this.state());
  }
  private card(id: ManagedApprovalId): Card | undefined {
    return this.cards.find((c) => c.approval.id === id);
  }
  /** Still the same approval at the same revision: an answer for it still applies. */
  private same(card: Card): boolean {
    return !this.disposed && this.cards.includes(card);
  }

  /** The shell's one authority follower feeds this. No second follower exists. */
  observe(observation: AuthorityObservation) {
    if (this.disposed) return;
    const view = observation.state === 'observed' ? observation.view : null;
    if (view?.connection === 'connected' && view.authority) {
      this.link = 'connected';
      // Something changed at the authority: what a list did not have, it may have now.
      if (this.head !== view.authority.last_sequence) this.triedFor = null;
      this.head = view.authority.last_sequence;
      this.restarts = view.authority.restarts;
    } else {
      const settling =
        observation.state === 'pending' ||
        (view !== null &&
          view.reason === null &&
          (view.connection === 'connecting' ||
            view.connection === 'synchronizing'));
      this.link = settling ? 'connecting' : 'offline';
      // Nothing read on a connection that is gone is taken for a pending approval:
      // what there is to decide is read again from whoever answers next.
      this.forget();
    }
    this.publish();
    this.sync();
  }
  private forget() {
    this.generation++;
    this.reading = false;
    this.wanted = false;
    this.shownHead = undefined;
    this.readFailure = null;
    this.cards = [];
    this.recent = [];
    this.partial = false;
    this.rest();
  }
  /** The Approvals view is shown. */
  setVisible(visible: boolean) {
    if (this.disposed || this.visible === visible) return;
    this.visible = visible;
    this.sync();
  }
  /** A turn is being followed: an approval may be asked for at any moment. */
  setWatching(watching: boolean) {
    if (this.disposed || this.watching === watching) return;
    this.watching = watching;
    this.sync();
  }
  /**
   * A reading of a turn named an approval that is not shown. It is read now, once: if
   * the authority's list does not have it either, it is not asked about again until
   * something changes.
   */
  want(id: ManagedApprovalId) {
    if (this.disposed || this.link !== 'connected' || this.card(id)) return;
    if (this.triedFor === id) return;
    this.triedFor = id;
    this.sync(true);
  }

  private stale(): boolean {
    if (this.shownHead === undefined && this.readFailure === null) return true;
    if (this.triedRestarts !== this.restarts) return true;
    if (this.triedHead === this.head) return false;
    return (
      this.shownHead === undefined ||
      sequence(this.shownHead) < sequence(this.head)
    );
  }
  /** Reads the approvals when what is shown is behind, and never twice at once. */
  private sync(force = false) {
    if (this.disposed || this.link !== 'connected') return;
    if (!force && (!(this.visible || this.watching) || !this.stale())) return;
    if (this.reading) {
      this.wanted = true;
      return;
    }
    const generation = ++this.generation;
    this.triedHead = this.head;
    this.triedRestarts = this.restarts;
    this.reading = true;
    this.wanted = false;
    this.publish();
    const settle = (apply: () => void) => {
      if (this.disposed || generation !== this.generation) return;
      this.reading = false;
      apply();
      this.publish();
      const again = this.wanted;
      this.wanted = false;
      if (again) this.sync(true);
    };
    this.client.list().then(
      (list) => settle(() => this.show(list)),
      (error) =>
        settle(() => {
          // What was shown is behind an authority that could not be read: it goes.
          this.cards = [];
          this.recent = [];
          this.shownHead = undefined;
          this.readFailure = failureOf(error);
          this.rest();
        }),
    );
  }
  private show(list: ApprovalList) {
    this.shownHead = list.head;
    this.readFailure = null;
    this.recent = list.recent;
    this.partial = list.partial;
    this.cards = list.pending.map((detail) => {
      const known = this.card(detail.approval.id);
      // The same approval at the same revision: what was read of its files and what
      // was asked of it still hold.
      if (known && known.approval.revision === detail.approval.revision) {
        known.approval = detail.approval;
        known.workspace = detail.workspace;
        known.command = detail.command;
        if (known.decision?.status === 'failed') known.decision.reread = true;
        return known;
      }
      return {
        approval: detail.approval,
        workspace: detail.workspace,
        command: detail.command,
        files: detail.files,
        shown: detail.file ? [fileCard(detail.file)] : [],
        listing: false,
        decision: null,
        intent: null,
      };
    });
    this.schedule();
    for (const card of this.cards) this.list(card, FILES_AT_ONCE);
  }

  /** Reads again whatever is shown. */
  refresh() {
    this.readFailure = null;
    this.triedFor = null;
    this.sync(true);
  }

  private rest() {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }
  /** One timer, for the deadline that comes first: what became of it is read then. */
  private schedule() {
    this.rest();
    const deadlines = this.cards
      .map((card) => Date.parse(card.approval.expires_at))
      .filter((time) => !Number.isNaN(time));
    if (deadlines.length === 0) return;
    const wait = Math.max(0, Math.min(...deadlines) - Date.now());
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.sync(true);
    }, wait + EXPIRY_GRACE_MS);
  }

  /** Reads the first page of files not read yet, `count` at most, one after another. */
  private list(card: Card, count: number) {
    if (card.listing || card.files === null) return;
    const next = () => {
      const index = card.shown.length;
      if (!this.same(card) || count <= 0 || index >= card.files!) {
        card.listing = false;
        this.publish();
        return;
      }
      count--;
      this.client.read(card.approval.id, index, 0).then(
        (detail) => {
          if (!this.same(card)) return;
          // Not the next file of this change set: nothing is shown in its place.
          if (
            detail.file === null ||
            detail.files !== card.files ||
            card.shown.length !== index
          ) {
            card.listing = false;
            this.publish();
            return;
          }
          card.shown.push(fileCard(detail.file));
          this.publish();
          next();
        },
        () => {
          // The files that were read stay; the rest can be asked for again.
          card.listing = false;
          this.publish();
        },
      );
    };
    if (card.shown.length >= card.files) return;
    card.listing = true;
    next();
  }
  /** Reads more files of a change set than were read by themselves. */
  moreFiles(id: ManagedApprovalId) {
    const card = this.card(id);
    if (this.disposed || this.link !== 'connected' || !card) return;
    this.list(card, FILES_AT_ONCE);
    this.publish();
  }
  /**
   * Reads the rest of one file's diff, page after page, to its end. The authority
   * bounds a diff, so this is a bounded number of reads.
   */
  more(id: ManagedApprovalId, index: number) {
    const card = this.card(id);
    const file = card?.shown.find((f) => f.index === index);
    if (this.disposed || this.link !== 'connected' || !card || !file) return;
    if (file.next === null || file.reading) return;
    file.reading = true;
    file.failure = null;
    this.publish();
    this.page(card, file);
  }
  private page(card: Card, file: FileCard) {
    this.client.read(card.approval.id, file.index, file.next).then(
      (detail) => {
        if (!this.same(card)) return;
        // The page that was asked for, of the file that was asked for, or nothing.
        if (
          detail.file === null ||
          detail.file.path !== file.path ||
          detail.file.bytes !== file.bytes
        ) {
          file.reading = false;
          file.failure = { code: 'internal', retry: false };
          this.publish();
          return;
        }
        file.text += detail.file.chunk;
        file.next = detail.file.next;
        if (file.next !== null && this.link === 'connected') {
          this.publish();
          this.page(card, file);
          return;
        }
        file.reading = false;
        this.publish();
      },
      (error) => {
        if (!this.same(card)) return;
        file.reading = false;
        file.failure = failureOf(error);
        this.publish();
        // It may no longer be there to read: what the approval is now is read.
        this.sync(true);
      },
    );
  }

  /**
   * Asks the authority for one decision on one approval, at the revision that is shown.
   * One at a time: while a decision is pending, or failed and neither retried nor
   * dismissed, nothing else is sent for that approval.
   */
  decide(id: ManagedApprovalId, kind: DecisionKind) {
    const card = this.card(id);
    if (this.disposed || this.link !== 'connected' || !card) return;
    if (card.decision !== null || !card.approval.decidable) return;
    card.intent = {
      id: this.newIntent(),
      kind,
      revision: card.approval.revision,
    };
    this.dispatch(card);
  }
  /** The failed decision again, whole: the same identity and the same decision. */
  retry(id: ManagedApprovalId) {
    const card = this.card(id);
    if (this.disposed || this.link !== 'connected' || !card?.intent) return;
    if (card.decision?.status !== 'failed' || !card.decision.failure?.retry)
      return;
    this.dispatch(card);
  }
  /**
   * Lets go of a decision that failed, once the approval was read again and is still
   * there to decide. What the user asks next is another action.
   */
  dismiss(id: ManagedApprovalId) {
    const card = this.card(id);
    if (this.disposed || card?.decision?.status !== 'failed') return;
    if (!card.decision.reread) return;
    card.decision = null;
    card.intent = null;
    this.publish();
  }
  private dispatch(card: Card) {
    const intent = card.intent!;
    card.decision = { kind: intent.kind, status: 'pending', reread: false };
    this.publish();
    const sent =
      intent.kind === 'approve'
        ? this.client.approve(intent.id, card.approval.id, intent.revision)
        : this.client.deny(intent.id, card.approval.id, intent.revision);
    sent.then(
      () => {
        if (!this.same(card) || card.intent !== intent) return;
        // The authority committed it. What the approval is now is read: it stays
        // shown as being decided until a list says what became of it.
        this.sync(true);
      },
      (error) => {
        if (!this.same(card) || card.intent !== intent) return;
        card.decision = {
          kind: intent.kind,
          status: 'failed',
          failure: failureOf(error),
          reread: false,
        };
        this.publish();
        // Refused, or not known to be taken: either way the approval is read again.
        this.sync(true);
      },
    );
  }

  dispose() {
    this.disposed = true;
    this.rest();
  }
}
