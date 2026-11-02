# Provider capability reference

Current implementation and qualification live in [STATUS](../STATUS.md), including
the later Claude/Codex Arsenal evidence. This reference defines how to interpret
provider capabilities; it is not a second dated implementation matrix.

## Separate the integration surfaces

| Surface                 | Meaning                                                                                                | What it does not establish                                                              |
| ----------------------- | ------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------- |
| Configuration adapter   | Wyss can read or write a supported provider file/scope with its version-specific checks.               | Execution mediation or provider consumption of that file.                               |
| Native launcher/profile | The original CLI is launched with supported settings and its own permissions/sign-in.                  | An attested Managed profile or a protected human approval principal.                    |
| Arsenal exclusion       | A qualified build receives per-session exclusions and the selection is recorded.                       | Filesystem denial, exclusion of system/plugin skills, or actual use of included skills. |
| Native delegation       | Wyss starts and supervises a provider-specific worker under its delegation contract.                   | Admission of Claude Managed, a B1 production boundary or F8 capacity above one.         |
| Managed adapter         | A qualified structured provider profile is mediated through authority lifecycle, leases and approvals. | Full Native feature parity or automatic permission for hooks/MCP/plugins/subagents.     |

## Observation labels

- **Configured:** declared in a source file, selection or profile.
- **Requested:** Wyss sent the option through its qualified launch/protocol path.
- **Observed:** the provider reported the setting or exposed capability for that
  session/build; identify exactly what was observed.
- **Used:** separate evidence that the provider actually invoked/read the capability.
- **Unknown/unsupported:** no reliable observation or qualified integration exists.

CLI syntax and a catalogue entry do not prove account entitlement or that a
particular requested model/effort answered a turn. Claude delegation init does not
confirm effort; Codex delegation checks its session record after execution. Native
provider subagents are not Wyss coordinator steps.

## Configuration and exclusion rules

Supported account/project authoring must preserve context, source validation,
outside-edit checks and kind-specific recovery. File-write success does not prove
live reload or consumption by an already running provider.

Claude Arsenal overlays and Codex per-session `skills.config` exclusions are
qualified for the exact builds recorded in STATUS. Codex uses `--no-daemon` and
exact `SKILL.md` paths, includes nested skills, preserves account-disabled entries
and does not emit `enabled=true`. System/plugin skill exclusions are not implied.
Arsenal imports and [sync](arsenal-sync.md) do not change existing sessions.

## Qualification of a changed surface

Record the exact provider/build, mode/profile, configuration/authentication route,
positive workflow, denied unsupported paths, observed values, restart/cancel and
resource behavior. Keep independent review and explicit acceptance for changed
security boundaries distinct from successful fixtures.

Native checks preserve the original sign-in, settings and features actually used.
Managed checks cover every unsupported effect route, including hooks, plugins,
MCP, background tools, network and provider subagents. An unavailable Managed
feature has an explicit reason; a Native alternative requires a new explicit
launch and is never an automatic fallback.

See [delegation](delegation.md), the
[Managed contract](managed-runtime.md) and
[current decisions](../DECISIONS.md) for their individual boundaries.
