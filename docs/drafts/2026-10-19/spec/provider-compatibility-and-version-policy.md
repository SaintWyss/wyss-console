# Provider compatibility, capability detection and support policy

**Status:** Compatibility policy proposed from architecture [06](../../../architecture/06-AGENT-ADAPTERS.md), 07, 12 and 18. Does **not** claim that untested provider versions work.

## Known local evidence as reported 2026-10-19

| Runtime | Reported version | Verified operations according to owner | Explicit limits |
| --- | --- | --- | --- |
| Claude Code | 2.1.303 | Native launch/PTY, live input, configuration inventory, hook/skill toggle and restore, session profile `--model` / `--settings` overlay, real first-screen smoke | Real `rtk hook claude` not directly mutated in latest smoke; effects of all live reloads unverified |
| Codex | 0.159.0 | Native launcher/PTY, configuration inventory, user MCP toggle exact undo, skill on/off via provider `skills/list`, first-screen smoke | Individual Codex hooks remain read-only; active-session skill reload not verified |
| OpenCode | Not independently tested in reported local F4 | None declared implemented | Requires F4a version detection and real config format verification |
| Hermes Agent | Not installed/verified in original architecture | None declared implemented | Requires consent to inspect/install and F4b adapter tests |
| Wyss Agent | Product strategy undecided | Not a verified Console provider | F12 comparison and owner DN-06 |

Reported version-specific features must be revalidated after provider upgrades; no automatic claim for future major or minor versions.

## Capability status vocabulary

`verified` = observed against named real provider version and recorded test. `documented` = vendor documentation only, not tested. `predicted` = derived from current file/plan rather than actual running provider. `unavailable` = proven unsupported. `unverified` = unknown, no action allowed when safety-relevant. `stale` = previous provider generation/report.

A provider adapter declares capability independently for:
- Installation detection, version and binary resolution.
- Native launch, launch-model flag, per-session overlay, CLI restart/resume.
- Configuration inventory per global/project scope; active/disabled/unknown.
- Reversible hook, skill, rule, MCP switch and semantics of running-session reload.
- Model catalogue, configured model, requested model, actual reported model, effort metadata.
- Provider-own configuration writer from closed allowed commands.
- Managed adapter attestability (default **false**, distinct from native launch).

Provider entries must not be marked `verified` by using the presence of an old fixture alone.

## Version negotiation and degradation

1. Detect CLI without executing arbitrary shell commands from the UI.
2. Parse version with provider-specific grammar and bounded output.
3. Select explicitly tested capability profile for that version.
4. Disable risky mutation if unknown, but allow safe read/launch only when their contracts remain verified.
5. Attach evidence provenance to UI. If version changed since plan, rebuild the plan.
6. Record supported version matrix in release notes; upgrade provider triggers focused integration smoke.

## Fixture minimum per provider

- Correct and malformed global/project config.
- Read-only/unsafe ancestor/symlink/ACL paths.
- Comments/ordering/duplicate keys for TOML/JSONC as applicable.
- Secret-bearing entries and impossible-to-classify content.
- Provider rewrite concurrent with switch, crash and undo.
- Installed/uninstalled/unsupported-version detection.
- Running-session nonreload or reload fixture where observed.
- MCP or hook start side effect and user consent boundary.
- All 5 model fact classes M1–M5 in reported and unavailable states.

## Release documentation

Show supported provider/version and capability pair, not just a provider logo. "Installed" does not mean authenticated; "model listed" does not mean entitled; "global hook enabled" does not mean managed Inference may execute. No unsupported compatibility promise from this spec.
