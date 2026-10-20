# ADR 0048: Model catalogue view, `model/rerouted` and native effort selection (F6)

Status: Accepted within Santiago's 2026-10-19 product-completion authorisation
(implements ADR 0031 §4.2 and §4.4 with option A, as ADR 0043 accepted; independently reviewed
before it is called a protection, see the acceptance record)
Date: 2026-10-20

Builds on ADR 0043. Vocabulary and rules are those of `docs/architecture/12-MODEL-MANAGEMENT.md`:
**offered** (a runtime's list), **configured** (an agent's file), **selected** (what a session
was started with) and **observed** (what a runtime reported) are four facts and are never merged
or filled from one another (AD-16).

## What changed

1. **The managed catalogue is shown, with what the runtime itself said about each entry.** The
   authority already held the descriptors; it now also keeps, entry for entry, a `ModelOffer`
   (default mark, default effort, supported efforts, replacement identifier and retirement date,
   all bounded provider words, never prose) and whether the single listing it took was
   complete. `runtime/model/list` carries them as optional fields, so an authority of an earlier
   version still decodes and the window says "not reported by this authority version". Provider
   upgrade copy, Markdown, links and descriptions are never read. Availability stays `unknown`
   and is not carried to the window at all: **a listing is not entitlement**, and the page says so.
2. **A read-only window command, `managed_model_catalogue`,** takes no argument and answers the
   catalogue, or `not_ready` when the managed runtime is not Ready (a catalogue of a stopped
   runtime is never shown), with the runtime's version, its mode, whether it is complete and the
   time of the read. The window cannot name a runtime, model or page.
3. **`model/rerouted` is tolerated, for exactly one case.** The classifier accepts it only when
   the object has exactly the five fields of the schema (`threadId`, `turnId`, `fromModel`,
   `toModel`, `reason`), the ids parse, both models are provider identifiers within bounds
   (printable, 1–256) and the reason is a bounded word, mapped to a closed `high_risk_cyber_activity`
   or `unrecognised`. The turn controller then accepts it **only** for the running turn of the
   current generation: another thread, another turn, a turn the provider has not named yet, or
   no turn is `inference_isolation_violation` as before. Anything malformed, in either profile,
   is the same violation, and no other notification is let through by it. An accepted reroute
   updates the in-memory attachment observation (option A): the destination model, `source:
turn_rerouted`, where it came from, the closed reason and the turn; provider and effort the
   thread answer reported are kept, not inferred or dropped. Nothing is stored, no event type or
   vocabulary changes, no audit record is added, and no stream event is added for a valid one.
   What later turns report is not documented (OQ-50) and the window says so.
4. **Native selection, per provider.** A profile now holds an optional `effort` beside its model,
   both opaque provider words passed at start and never checked here (`model_unchecked` is said
   in the form and the Models view). Claude Code gets `--model` / `--effort`, and only the levels
   its own `--help` names (`low`, `medium`, `high`, `xhigh`, `max`); Codex gets `-m` and
   `-c model_reasoning_effort="<word>"`, its documented configuration override, for that session
   only (the word is shape-checked: letters, digits, `. _ -`, ≤ 32, no leading dash, so it cannot
   end the string or start a flag). A body without an effort serializes without the key, so every
   stored profile keeps its bytes and its version hash. The workspace shows, per native session,
   the model and the effort asked for, the model and effort the agent's files hold, "in use: not
   reported by the agent", and Claude Code's own `/model` hint (Codex's is not verified, so none
   is offered).
5. **A Models view** shows the four facts in four lists, each with where it comes from; it only
   reads (the managed catalogue once on first show and again on request, the inventory the
   Configuration view reads, the sessions the Sessions view reads, the profiles).

## Not done, on purpose

- **No managed model or effort choice.** `thread/start`, `thread/resume` and `turn/start`
  parameters are unchanged (ADR 0014/0015); no field a client can send is added. That would be
  DN-42, with its own review. SG-12 is untouched.
- No usage, rate-limit or cost figures (DN-43). No durable model history (option C).
- Hidden catalogue entries are not requested (`includeHidden: false`, as before).

## Compatibility

Window page schema stays 4: the three reroute fields appear only for a rerouted observation, so
every earlier shape is byte-identical. An older desktop with a newer authority would refuse a
rerouted observation it cannot read; both ship in one package.
