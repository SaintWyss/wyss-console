# F1b: can managed Inference be isolated from the user's hooks? (experiments E1 to E8)

> Current execution policy (Santiago, 2026-10-19): the eligible remaining roadmap is
> authorised. Real development/acceptance uses the owner's normal computer and environment;
> no disposable HOME, private keyring/bus, VM, container or artificial user installation.
> Older isolated procedures/results below are historical and must not be repeated.
> Unit fixtures and intrinsic production Managed security isolation remain valid.
> Current implementation, acceptance evidence and accepted ADRs take precedence over
> historical planning statuses. See `../CHECKPOINT.md` (from architecture/acceptance/research).

Run 2026-10-19 on `saint-acer` with the installed Codex 0.159.0 (`codex app-server`, JSON-RPC over
stdio), in disposable homes under `/tmp` (`HOME` and `CODEX_HOME` both set; the session bus and
`XDG_RUNTIME_DIR` removed so nothing could reach a real wallet). The owner's real `~/.codex` was
not read or written: `config.toml` hash `bf11268d…` before and after. No credential of the owner
was used; the only credential is the string `sk-test-SYNTHETIC-f1b-0000`. Repeat with
`python3 scripts/research/f1b/run.py` (E1, E2, E3, E4, E7); E8 is the `bwrap` run below.

The host's `/etc/codex/requirements.toml` was left visible, as it is for Inference today: it pins
`features.hooks = true` and defines no managed hook (`allow_managed_hooks_only = false`).

## Results

| #   | Question                                      | Result                                                                                                                                                                                                                                                                                                                      | Control                                                                                      | Verdict                                                  |
| --- | --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| E1  | Does a private `CODEX_HOME` hide user hooks?  | `hooks/list` under the private home: **0** entries                                                                                                                                                                                                                                                                          | Same user hook under the user home: 1 entry (`sessionStart`, enabled)                        | **Pass**                                                 |
| E2  | Does the existing check pass under policy?    | Policy visible: `featureRequirements.hooks = true`, `features.hooks` effective `true` even with `-c features.hooks=false` (the existing override is defeated by the pin, as K3 says), **0 enabled definitions** seen, so `zero_external_hooks` has nothing to count                                                         | A user hook in the home: 1 enabled definition (the check would fail)                         | **Pass** (a managed hook in `/etc/codex` not exercised¹) |
| E3  | Can the credential cross without a namespace? | `auth.json` in the private home was a link to a synthetic file; a local API-key login wrote **through** the link: link survived, target received the key                                                                                                                                                                    | —                                                                                            | **Partial**²                                             |
| E4  | Does a trusted project's `.codex` reach it?   | Project hook loaded under the user home that trusts the project; **not loaded** under the private home                                                                                                                                                                                                                      | Trusted in the private home: loaded                                                          | **Pass**                                                 |
| E5  | Does every other attestation still hold?      | `config/read` and `configRequirements/read` answer under the private home and reflect the requirement pin; the authority's own preflight was **not** run against an isolated start                                                                                                                                          | —                                                                                            | **Not run**                                              |
| E6  | Does a real turn complete, and a resumed one? | **Not run**: a turn calls a model, which needs a real credential and spends the owner's quota                                                                                                                                                                                                                               | —                                                                                            | **Not run**                                              |
| E7  | Where do threads live?                        | Under `CODEX_HOME`: its own `state_5.sqlite`, `logs_2.sqlite`, `installation_id`, skills; a thread started in one home is "no rollout found" from another home (and from its own before a first turn, so the rollout file is written by a turn). Today's Inference threads are in the user's home, so bindings do not carry | —                                                                                            | **Stated**: a private home breaks resume of old threads  |
| E8  | Do namespace and visible policy coexist?      | Under `bwrap` (I4, `/etc/codex` visible): the runtime reports `featureRequirements.hooks = true`, 0 hooks. With `--tmpfs /etc/codex` (I2): `requirements: null`, the policy is invisible                                                                                                                                    | I2 is the control: it differs, so visibility is decided by the namespace and not by the home | **Pass**                                                 |

¹ `/etc/codex` is root-owned and was not changed. The managed-hook case is by the design's own
argument (a definition exists, the check fails, Inference is unavailable and says why).
² An API-key login is not a token refresh. It shows that Codex writes `auth.json` in place through
a link, which is what K8 says of the refresh; the refresh itself needs a real expiring token and was
not exercised.

## Recommendation for DN-03

**I4: a private home inside a namespace that leaves the host's `/etc/codex` visible**, as the design
ranked it. E1, E2, E4 and E8 support it with controls that fail where they must: user hooks and
trusted-project hooks do not reach the isolated runtime, the administrator's pin is still obeyed and
cannot be bypassed by the override, and a replaced `/etc/codex` (I2) is the only way a policy is hidden.

It is **not proven** until the remaining parts run with a credential path the owner chooses:

- E3 with a real refresh (or the existing descriptor path of Local Tools, ADR 0016, which I4 reuses);
- E5, the authority's full preflight on an isolated start, and every attestation of ADR 0015 unchanged;
- E6, one turn and one resume, with a throw-away or rate-limited credential;
- the decision on E7: a private home cannot resume threads bound in the user's home, so either
  existing bindings are declared ended at the switch, or the thread store is made shared, which is
  a new security surface.

Until those pass, G-INF is **not implemented** and the authority's attestation is unchanged: the
managed-Inference hook block stays.

## What this does not claim

One Codex version (0.159.0) and one machine. Behaviour that is undocumented (the application's
in-place write, the hook-trust state) is observed, not promised, and a Codex update must repeat
`run.py`.
