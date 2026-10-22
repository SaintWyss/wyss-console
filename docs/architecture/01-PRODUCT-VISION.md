# 01 — Product vision

> **2026-10-22 consolidation:** this document retains dated design/evidence and detailed
> contracts. Its present-tense status/decision tables describe their stated baseline,
> not necessarily current implementation. Read the [current product set](../product/README.md),
> [verified gaps](../product/implementation-status.md) and [requirement crosswalk](../product/traceability.md)
> first. F8 is active P2/V8, ADR0051 Accepted, cap1; P1 remains available. New product
> direction does not supersede accepted security ADRs or turn proposed tests into PASS.

Expands [`docs/PRODUCT.md`](../PRODUCT.md), which rules where the two differ.
Marks (`[R]`, `[D]`, `[O]`, `[P]`, `[N]`) are defined in the [index](00-INDEX.md#evidence-classes).

## 1. Definition

> Wyss Console is a fast, native, extensible terminal and visual control centre for AI
> development environments. It gives one place to manage agents, their real configuration
> files, rules, hooks, skills, profiles, execution, sessions, permissions and observability.

Three properties follow and are not negotiable:

1. **It is a terminal first.** It is useful with no agent installed, no authority running and
   no network. An agent feature that makes the terminal slower or heavier at rest is a defect.
2. **What it changes is real.** A change made in it is a change to the file the agent reads.
   The agent sees it when started anywhere else; a change made anywhere else is seen in it.
3. **It says only what it knows.** Persisted, provider-resolved and session-effective state are
   three things. An inferred or unknown state is never drawn as a verified one.

It is not a chat application with a terminal attached, and not a settings screen.

## 2. Where it comes from

The owner worked in Warp with Codex, Claude Code and other tools, under a methodology of his
own (`[O]`): global and per-project instructions, engineering and security rules, validation
hooks, skills, model routing and working profiles, on several repositories and two computers.
Ten recurring problems define the product (`[O]`, owner's statement of 2026-10-12):

| #   | Problem                                                                   | Answered by                  |
| --- | ------------------------------------------------------------------------- | ---------------------------- |
| P1  | Switching between agents was cumbersome                                   | 05 (launcher), 13 (overview) |
| P2  | Sessions had no unified visual management                                 | 05, 09, 13                   |
| P3  | Agent configuration was spread over different formats and directories     | 06, 07                       |
| P4  | Global and project rules were hard to administer consistently             | 07, 08                       |
| P5  | Hooks and security controls had to be on for one task and off for another | 08, 09                       |
| P6  | Managing the methodology meant editing files by hand                      | 07, 08                       |
| P7  | There was no reliable picture of which configuration was active           | 07 §3 (three states), 13     |
| P8  | Different working profiles were wanted                                    | 09                           |
| P9  | Several agents had to coexist in one environment                          | 05, 09                       |
| P10 | The terminal itself had to stay fast, light and comfortable               | 05, 15                       |

## 3. Who it is for

| User                                          | Today                                  | Target                                                    |
| --------------------------------------------- | -------------------------------------- | --------------------------------------------------------- |
| The owner, on CachyOS with KDE, two computers | The only user `[R]` (ADR 0027)         | First user of every phase                                 |
| Another developer using one or more agents    | Not served: no installer, one platform | Served after a distribution decision (18)                 |
| A team sharing a methodology                  | Not served                             | Out of scope of this set; noted in 18 §8 as a later study |

Nothing in the design may depend on the owner's paths, user name, host names or provider
settings (`FR-DIST-01`). His environment is the first test case, not an assumption.

## 4. Two planes

Approved as decision D1 (ADR 0028 §1) `[R]`.

| Plane       | What runs                                 | Configuration                                          | Who decides what it may do            | Built today                                             |
| ----------- | ----------------------------------------- | ------------------------------------------------------ | ------------------------------------- | ------------------------------------------------------- |
| **Native**  | An agent's own CLI in a terminal          | The user's real global and project files, or a profile | The agent and the user who started it | Only by hand: the user types the command in a tab `[R]` |
| **Managed** | Codex under the authority, `wyss-control` | The isolated one the authority fixes                   | The authority: leases and approvals   | Yes `[R]`                                               |

A native session is a human-operated terminal process, never a managed agent (ADR 0028 §1,
ADR 0002 `:63-70`) `[R]`. The Console may start it, watch it and configure what it reads. It
grants it nothing and contains nothing.

## 5. What "real-time visual control" means

The phrase is the owner's. In this set it has an exact meaning:

- **Real**: the provider's own file is the thing changed (07 §5).
- **Time**: a change the user makes is applied at once; a change made elsewhere is shown within
  a bounded delay (07 §7, `NFR-PERF-09`). It does not mean that a running agent has adopted
  it: that is a separate, per-provider fact (06 §3, 07 §6).
- **Visual**: every element shows where it lives, what its file says, what the provider
  resolved and which sessions use it (13 §5).
- **Control**: every action has a defined effect, a defined failure and a way back (08).

## 6. Core user journeys

Each is specified step by step, with its failure paths, in [13 §8](13-UX-UI-SPECIFICATION.md).

| #   | Journey                                               | First phase that makes it possible                          |
| --- | ----------------------------------------------------- | ----------------------------------------------------------- |
| J1  | Open Wyss Console as an ordinary terminal             | Built `[R]`                                                 |
| J2  | Launch Claude Code with the real global configuration | F3 (today: by hand in a tab)                                |
| J3  | Disable a global hook                                 | F2a (Claude Code); G-INF for enabling a Codex hook          |
| J4  | Verify the change outside Wyss                        | F2a                                                         |
| J5  | Restore the hook                                      | F2a                                                         |
| J6  | Detect a manual external change                       | F1                                                          |
| J7  | Change profiles                                       | F3                                                          |
| J8  | Run two native agents in parallel                     | F3 (today: two tabs, unobserved)                            |
| J9  | Run secure managed Codex                              | Built `[R]`, verified with real Codex `[O]`                 |
| J10 | Inspect permissions and approvals                     | Built `[R]`                                                 |
| J11 | Select or inspect models                              | F6                                                          |
| J12 | Recover from configuration conflicts                  | F1 (see), F2a (resolve)                                     |
| J13 | Recover from a crash                                  | Built for the authority `[R]`; F2a for configuration writes |
| J14 | Resume work from another computer                     | F9 and F10                                                  |
| J15 | Connect remotely from a phone                         | F10                                                         |

## 7. What success looks like

The product has succeeded when its owner can answer these without opening a file or a second
tool, and the answer is true:

1. Which agents are running, where, on which project, in which plane?
2. Which rules, hooks, skills and MCP servers does each of them actually use?
3. What did I change today, on which computer, and how do I undo it?
4. If I turn this hook off, which sessions feel it now, which at their next start, and does
   anything else stop working?
5. Which model answered this turn?
6. What is waiting for my decision?

And when a second developer, on a clean machine, can install it, see his own agents'
configuration without having granted anything, and trust that nothing was changed that he did
not confirm.

## 8. Not goals

| Not a goal                                                       | Why                                                           |
| ---------------------------------------------------------------- | ------------------------------------------------------------- |
| Replacing the agents' own CLIs with implementations of our own   | The agents are the providers' products (06)                   |
| An IDE, a file manager, a Git client                             | A terminal already runs those                                 |
| A second configuration format that agents must be taught to read | Property 2: the provider's file is the truth                  |
| Capturing a model's private reasoning                            | Not available and not wanted (ADR 0015) `[R]`                 |
| Mediating what a native agent does                               | That is the managed plane; mixing them hides what is enforced |
| A remote desktop                                                 | Remote control is of terminals, sessions and approvals (16)   |
| Forking Hermes now                                               | Decision D9: the original upstream first (17)                 |
