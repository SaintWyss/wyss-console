# 0030 — Corrections to two security statements of ADR 0028

Status: **Proposed** on 2026-10-13. Not accepted. It changes no code, no permission and no
capability of the window. It authorises no implementation.

It was written by the author of the architecture set and has not been reviewed by anyone else.
On 2026-10-13 the owner said he supports correcting the inaccurate security claims of ADR 0028.
Formal acceptance waits for a review of the exact text below.

Date: 2026-10-13

## Context

ADR 0028 is accepted. Two of its sentences claim more than the design can deliver. The
independent review of 2026-10-12 raised both (`docs/architecture/25-INDEPENDENT-REVIEW.md`,
IR-01 and IR-08). An accepted ADR is not rewritten to look as if it had been right
(`docs/architecture/21-ADR-RECONCILIATION.md` §5): this ADR names each sentence, says why it
does not hold, and gives the statement that replaces it. The reasoning of ADR 0028 around them
stands.

## Correction 1 — What the native confirmation protects (ADR 0028 §5)

### The sentence

> **What that check is.** It protects the user's configuration from a compromised page and
> from a slip. (`0028`, section 5)

### Why it does not hold

The window is one web view with one capability list.

| Fact                                                                                                                        | Evidence                                                       |
| --------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| One window, `main`, with one capability file                                                                                | `src-tauri/capabilities/main.json`                             |
| That list includes creating a terminal and writing into one                                                                 | The same file: `allow-create-terminal`, `allow-terminal-write` |
| A terminal is the user's shell, with the user's rights                                                                      | ADR 0005                                                       |
| The page's content policy allows scripts from the application itself only, no remote origin, no frame; styles may be inline | `src-tauri/tauri.conf.json:26`                                 |
| The window has no general file access and no opener                                                                         | The same capability file                                       |

Code that runs in the page, by a defect in what the page renders or in a dependency, does not
need the configuration commands. It can write `sed`, `rm` or an editor command into a terminal
and change the same files from the user's shell, with no dialog. The native confirmation is on
a path such code has no reason to take.

### The statement that replaces it

> The backend's check, and the native dialog it opens, do two things. They prevent a slip: a
> change that weakens a control or brings back executable content is confirmed in a second,
> separate place that shows what the backend computed. And they prevent the configuration
> commands from applying a sensitive plan that the user did not confirm, whatever the page
> sends. They are **not a boundary against code running in the page**: that code can use a
> terminal. They are not a boundary against the user or another process of his either, as ADR
> 0018 already says.
>
> What stands between hostile content and the page is the page's content policy and the rule
> that configuration text and agent output are rendered as text, never as markup. Both are
> defence in depth. Neither is a boundary.

### The two things kept apart

| Protection against a mistake                                                             | Containment of hostile code                                                                        |
| ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| The native dialog; risk classified in the backend; the plan bound to a hash; no "always" | **None today** between the configuration view and the terminal                                     |
| Holds whatever the page does through the configuration commands                          | Would need the two to be separate web views, or separate processes, with separate capability lists |

### A real boundary, as an option

Not designed here and not decided (DN-22, alternative b; OQ-36).

| Step                                                                                                  | Effect                                                      | Limit                                                                           |
| ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------- |
| The configuration view in its own web view, whose capability list has no terminal command             | A defect in the configuration view cannot type into a shell | A defect in the terminal view still has a shell: that is what a terminal is     |
| The terminal view's list has no configuration command                                                 | A defect in the terminal view cannot apply a plan           | It can still edit the files from the shell                                      |
| Content from agents and from configuration files is rendered only in the view that has fewer commands | The riskiest content meets the smallest capability list     | Changes the structure of the window; unverified in the framework version in use |

### Consequences for later phases

| Phase                             | Consequence                                                                                                                                                                                                           |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F2a, F2v: mutations and the vault | Every safeguard of the configuration commands and of the vault's interface is described as protection against mistakes and against a defect confined to that view. None is described as containing a compromised page |
| F3a: the native launcher          | A start request from the page selects from a closed catalogue. That limits what the launcher's command can do; it does not limit a page that can already type the same command into a terminal                        |
| F5: authoring                     | Typing a hook's command in a native input keeps command text out of the page's requests. It is where separate web views would matter most, and is why the option is to be studied before F5                           |
| F10: remote                       | A remote client is a second page, on another device. Nothing here is to be read as protecting the host from it                                                                                                        |

## Correction 2 — A provider writing at the same moment (ADR 0028 §8)

### The sentence

> A provider writing between the last check and the rename cannot be excluded: it is detected
> on read-back and recoverable from the backup. (`0028`, section 8)

### Why it does not hold

| Half of the sentence          | Why it is wrong                                                                                                                                                                                                                                          |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| "detected on read-back"       | A rename replaces whatever is under the name. If the provider wrote in that interval, its write is gone, and reading the file back returns exactly what the Console wrote. Read-back confirms the Console's own bytes; it cannot show what they replaced |
| "recoverable from the backup" | ADR 0028's own section 7 keeps no copy of a whole file ("No whole configuration file is copied"). There is no backup of the file to recover from                                                                                                         |

### The statement that replaces it

> A provider takes no lock and may write its configuration file at any moment. **No protocol
> on an ordinary filesystem prevents a writer that does not cooperate from losing an update or
> causing one to be lost.** Wyss Console does not claim otherwise.
>
> A provider's file is written only by a protocol that (a) never replaces content it has not
> examined, (b) ends every ordering it can observe in a named, journalled outcome, and (c)
> states the orderings it cannot observe. A plain rename over the file is not such a protocol
> and is not used, as a first choice or as a fallback. Reading the file back verifies what was
> written and is never presented as detection of a concurrent write.
>
> Where a provider offers its own verified writer for a setting, that writer is used instead.

Which protocol meets (a) to (c) is **not decided by this ADR**. The candidate and its limits
are in `docs/architecture/07-CONFIGURATION-ENGINE.md` §5.4 and
`docs/architecture/28-CONCURRENCY-AND-FILESYSTEM-INTEGRITY.md`; the choice is decision DN-21.
What this ADR fixes is only that the sentence above no longer describes the design, and that a
plain rename is excluded.

## What is not changed

Everything else in ADR 0028: its decisions D1 to D10, the mutation pipeline as a sequence, the
round-trip precondition, the classification of risk in the backend, gate G-INF. ADR 0005 and
the terminal's capabilities. No capability list, content policy or permission is modified by
this ADR or by the documentation pass that produced it.

## What the owner is asked to decide

| #   | Question                                                                            | Proposed answer                                    |
| --- | ----------------------------------------------------------------------------------- | -------------------------------------------------- |
| C1  | Is the statement of correction 1 accepted in place of the sentence of ADR 0028 §5?  | Yes                                                |
| C2  | Is a boundary by separate web views to be studied before F5?                        | Yes, as a study (OQ-36); no commitment to build it |
| C3  | Is the statement of correction 2 accepted in place of the sentence of ADR 0028 §8?  | Yes                                                |
| C4  | Is a plain rename excluded as a fallback on filesystems without an atomic exchange? | Yes: no mutation of files there                    |

## References

- ADR 0028, sections 5, 7 and 8; ADR 0005; ADR 0018 `:110-121`.
- `docs/architecture/11-SECURITY-THREAT-MODEL.md` §4, CTL-27;
  `docs/architecture/22-DECISIONS-AND-OPEN-QUESTIONS.md`, DN-21, DN-22;
  `docs/architecture/25-INDEPENDENT-REVIEW.md`, IR-01, IR-08.
- `src-tauri/capabilities/main.json`; `src-tauri/tauri.conf.json:26`.
