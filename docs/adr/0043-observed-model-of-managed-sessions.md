# ADR 0043: The model a managed runtime reports (F6a, managed), accepting ADR 0031 option A

Status: Accepted (owner approved DN-09 with the transient observation, 2026-10-19)
Date: 2026-10-19

## Decision

- The answer to `thread/start` and `thread/resume` carries `model`, `modelProvider` and
  `reasoningEffort`. The adapter now keeps them as an `ObservedModel` (bounded printable words;
  missing or out-of-bounds means no observation, never a default) and the control core holds it
  **in memory beside the attachment**, gone with it (detach, runtime stop, authority exit). Nothing
  is written to SQLite or to the event log; no event vocabulary changes.
- It reaches the window as `observed_model` on a session of the page (schema version 4), only for
  an attached session, and the page says what it is: "Model in use: <id> (reported by the runtime
  when the session was started|resumed)", or "not reported by the runtime", or "not known while
  detached". Catalogue size (`model_count`) stays a count and is never shown as what a session uses.
- SG-12 is untouched: no field the client sends, no selection of a model on the managed plane. A
  managed model **choice** would need its own authority contract and is not done.
- `model/rerouted` still fails a turn closed as before: accepting it is option A's companion in
  ADR 0031 §4.2 and is a separate, reviewed change to the classifier.

## Native sessions

For a native session the window shows three different things and never merges them: the model the
**profile requests** (passed as `--model`/`-m`), the model the agent's **files configure**
(Configuration view), and "in use: not reported", because neither agent reports it without a hook or
a scrape of the terminal, which this product does not add.
