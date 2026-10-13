# Wyss Console: the product

This page says what Wyss Console is for and what its owner has asked of it. It is the first
document to read and the one the others answer to (see [Documents](#documents)). What is
actually built is in the [README](../README.md); this page says so where the two differ.

Status: approved by the owner on 2026-10-12, with the ten decisions recorded in
[ADR 0028](adr/0028-product-direction-and-agent-configuration.md), which is accepted.

## What it is

Wyss Console is a fast, light, native terminal and the control centre of a development
environment that works with AI agents. It shows and changes the real configuration of each
installed agent (rules, hooks, skills, MCP servers and settings), says which configuration
each session actually uses, and runs supervised work under a separate authority with leases
and approvals. The terminal works with no agent at all.

It is not a chat application, not only a terminal emulator, and not a settings screen: a change
made in it is a change to the file the agent reads, visible to that agent when it is started
anywhere else.

## Why it exists

Its owner works every day with Codex, Claude Code and other agents under a methodology of his
own: global and per-project instructions, engineering and security rules, validation hooks,
skills, model routing and working profiles, across several repositories and two computers. In
the terminal he used, switching agents and knowing or changing which rules and hooks were
active meant editing configuration files by hand and remembering what was on. Wyss Console
exists to give that environment one visible, real-time place of control.

## Two planes

| Plane       | What runs                                 | With which configuration                                               | Who decides what it may do                                |
| ----------- | ----------------------------------------- | ---------------------------------------------------------------------- | --------------------------------------------------------- |
| **Native**  | an agent's own CLI in a Console terminal  | the user's real global and project configuration, or a session profile | the agent itself: its permissions and its hooks           |
| **Managed** | Codex under the authority, `wyss-control` | the isolated configuration the authority fixes                         | the authority: leases and approvals, one action at a time |

The two are never shown as one. Every session says which plane it is in and which configuration
it uses. A change to the global configuration never makes a managed session claim to use it.

## Requirements the owner has stated

| #   | Requirement                                                                                                                                                                          | Built today                                                                  |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------- |
| R1  | A capable terminal on its own: low idle memory, fast start and input, correct resize and IME                                                                                         | Yes, unmeasured: no performance figure exists yet                            |
| R2  | Read and change the real global configuration files of the installed agents, in both directions: first switching what exists, then creating and editing, as far as each agent allows | No                                                                           |
| R3  | Show what is declared, what the agent resolved and what each running session uses, separately                                                                                        | No                                                                           |
| R4  | Never report a change as active because a file was written                                                                                                                           | No                                                                           |
| R5  | Security controls stay under the owner's control and never change silently                                                                                                           | No                                                                           |
| R6  | Global editing and independent per-project or per-session profiles, as two distinct features                                                                                         | No                                                                           |
| R7  | Start and supervise an agent from the Console, with profiles and observability                                                                                                       | No. An agent can be started by hand in a terminal tab                        |
| R8  | Codex, Claude Code, OpenCode and Hermes, through adapters that state what each can really do                                                                                         | Codex only, managed                                                          |
| R9  | Keep the managed execution boundaries exactly as they are                                                                                                                            | Yes                                                                          |
| R10 | Nothing tied to one person's paths, user name, host or provider settings                                                                                                             | Yes in the code; the product is built for one desktop                        |
| R11 | Native Codex runs the owner's real global hooks while managed Inference keeps working, independently of them                                                                         | No. Today managed Inference refuses to start while any Codex hook is enabled |

## Not now

Each of these is wanted and each needs its own decision before any work starts: several managed
runtimes or turns at once, handing work from one agent to another, showing the model of a
session, remote access from another device, a personal assistant of the owner's own, and
distribution to other users ([ADR 0027](adr/0027-local-product-no-distribution-packaging.md)
stands).

## Documents

| Order | Document                  | Answers                                                                                                 |
| ----- | ------------------------- | ------------------------------------------------------------------------------------------------------- |
| 1     | This page                 | What the product is and what has been asked of it                                                       |
| 2     | `docs/spec/`, per feature | The exact behaviour of a feature and how it is accepted                                                 |
| 3     | [`docs/adr/`](adr/)       | Technical decisions. A later one extends or supersedes an earlier one; an accepted one is not rewritten |
| 4     | [README](../README.md)    | What is implemented and verified, and nothing else                                                      |

A goal on this page does not by itself override an established security control. Changing one
takes an explicit technical decision, recorded in an ADR, and proof.

`wyss-agent-legacy/` (the archive of the original Wyss assistant), the base specification v0.1 and
the research and vision reports that preceded this page are inputs. What is taken from them
enters through an ADR.
