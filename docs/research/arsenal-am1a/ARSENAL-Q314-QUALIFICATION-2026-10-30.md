# Arsenal skill exclusion — focused qualification of Claude Code 2.1.314

Date 2026-10-30 · builder's record, not yet independently reviewed · run root
`~/.local/state/wyss-a0/20261030-arsenal-q314-140d01/` (raw evidence stays local).

## What was qualified

Exactly the mechanism Arsenal emits for a Native Claude Code start: `--settings <file>` where the
file holds only `skillOverrides`, every value `off`, every name an installed user or project skill.
Nothing else: no plugin, MCP, agent, hook or permission setting, and no addition of stored content.

Binary `2.1.314 (Claude Code)`, SHA-256
`4487042b4c4f5d88d4568f68ae98aae0b1ffae58864cf57fb868641bc5f84594`.

## Method

- The A0 `harness-v2` (`scripts/research/a0/harness-v2/`) copied into a fresh run root; the only
  change is the pinned binary (2.1.312 → 2.1.314) and the matching onboarding version in the
  fixture. Harness tree SHA-256 `2902a4295ae4c47d18b9ae7595b228832805b16c7358ec168d9dfce5bfb3d8b4`.
- bwrap `--unshare-all`, empty launcher environment, tmpfs HOME/work, synthetic key, loopback mock;
  the real HOME is never mounted. Preflight 34/34 (`evidence/q314/preflight-summary.json`).
- Cases pre-registered before the first launch (`evidence/q314/PRE-REGISTERED.json`), driven by
  `scripts/research/a0/qualify_314.py`. The oracle reads the first model request: the skill listing
  and the description canaries of each fixture skill. The overlay validator result is part of the
  verdict.

## Result

| Case      | Overlay (off)           | Mode        | Listed fixture skills                                       | Verdict |
| --------- | ----------------------- | ----------- | ----------------------------------------------------------- | ------- |
| Q-C0      | none (control)          | headless    | alpha, beta, gamma, proj                                    | PASS    |
| Q-S1-1..3 | beta, proj              | headless    | alpha, gamma (3/3)                                          | PASS    |
| Q-S2-1..3 | alpha, gamma            | headless    | beta, proj (3/3)                                            | PASS    |
| Q-H1      | beta, proj + user hooks | headless    | alpha, gamma; SessionStart and UserPromptSubmit hooks fired | PASS    |
| Q-S1-I    | beta, proj              | interactive | alpha, gamma                                                | PASS    |

The control lists every fixture skill and each overlay removes exactly the named ones, so the
overlay is what discriminates. Excluded skills' descriptions are absent from the request body; no
skill body was sent. Isolation held in every case: integrity, no leftovers, owned fixture files
unchanged, no evidence tampering. The interactive case tried the known `downloads.claude.ai` and
`github.com` connects, which the sandbox refused (same baseline as A0). Per-case result and
manifest digests: `evidence/q314/SHA256SUMS`.

## What this does not show

- Plugin skills: not affected by `skillOverrides` (provider documentation). Arsenal lists enabled
  plugins as "not changed" instead of claiming to hide them.
- Built-in skills: not switched off by Arsenal.
- Two concurrent sessions were not run. Each Native start writes its own overlay file in its own
  session folder and Claude Code has no shared daemon; interference is not expected, not proven.
- The owner's real settings were not loaded; the fixture user settings carry hooks and
  permissions only. The A0 failure modes (an invalid value drops the whole overlay; a misspelled
  name is a silent no-op) are why the desktop validates every name against the installed skills
  before a start, and why Arsenal states the application as "use not observed".
- Any other build. The record in `native/capabilities.rs` names this exact version line only.
