# Arsenal skill exclusion — focused qualification of Claude Code 2.1.319

Date 2026-11-07 · builder's record · run root `~/.local/state/wyss-a0/20261107-arsenal-q319-20c212/` (raw evidence stays local).

Claude Code updated itself from 2.1.316 to 2.1.319 on Santiago-PC. The record of 2.1.316 does not
cover another build, so the same qualification was repeated for this one.

- Binary `2.1.319 (Claude Code)`, SHA-256 `dabafec82ca1319fa43d8be36a10fc61b1d11821cb98ad5df04ad95e199b8eed`.
- Method: identical to [ARSENAL-Q316-QUALIFICATION-2026-10-31.md](ARSENAL-Q316-QUALIFICATION-2026-10-31.md): the
  harness-v2 tree of that run root with only the pinned binary, its digest and the onboarding version
  changed; bwrap `--unshare-all`, empty launcher environment, tmpfs HOME, synthetic key, loopback mock.
  Isolation preflight 34/34; host integrity unchanged.
- The host-control run (the same probe outside the sandbox, expected to fail) failed 17 checks
  instead of 19: `no_host_codex_daemon_dir` and `no_host_codex_socket_reachable` passed because no
  Codex daemon was running on the host. The sandbox side does not depend on it.
- Cases: the same nine, byte-for-byte the 2.1.316 `evidence/PRE-REGISTERED.json`, written before the
  first launch.

| Case      | Overlay (off)           | Mode        | Verdict |
| --------- | ----------------------- | ----------- | ------- |
| Q-C0      | none (control)          | headless    | PASS    |
| Q-S1-1..3 | beta, proj              | headless    | PASS ×3 |
| Q-S2-1..3 | alpha, gamma            | headless    | PASS ×3 |
| Q-H1      | beta, proj + user hooks | headless    | PASS    |
| Q-S1-I    | beta, proj              | interactive | PASS    |

The nine cases above prove Arsenal skill exclusion only. Profile flags were checked separately and
only for syntax: `--model`, `--effort` (low, medium, high, xhigh, max), `--permission-mode` (choices
include `manual`, `plan`, `acceptEdits`), `--dangerously-skip-permissions` and `--settings` are all
in this build's `--help`. No Native process was launched with them, so their effect, including full
access, is not observed.

The limits of the 2.1.314 record apply unchanged. saint-acer was offline and was not checked: it
qualifies only if it runs this exact build.
