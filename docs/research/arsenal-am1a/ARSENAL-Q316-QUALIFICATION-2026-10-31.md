# Arsenal skill exclusion — focused qualification of Claude Code 2.1.316

Date 2026-10-31 · builder's record · run root `~/.local/state/wyss-a0/20261031-arsenal-q316-27f090/` (raw evidence stays local).

Claude Code updated itself from 2.1.314 to 2.1.316 on this machine. The record of 2.1.314 does not
cover another build, so the same qualification was repeated for this one.

- Binary `2.1.316 (Claude Code)`, SHA-256 `fd13c70000fec3f2116dfbcbb6121210b4a29328bf0e46210a59dd594a67d824`.
- Method: identical to [ARSENAL-Q314-QUALIFICATION-2026-10-30.md](ARSENAL-Q314-QUALIFICATION-2026-10-30.md): the
  harness-v2 tree of that run root with only the pinned binary, its digest and the onboarding version
  changed; bwrap `--unshare-all`, empty launcher environment, tmpfs HOME, synthetic key, loopback mock.
  Isolation preflight 34/34 (the 19 host-control failures are the expected negative control).
- Cases: the same nine, pre-registered before the first launch (`evidence/PRE-REGISTERED.json`).

| Case      | Overlay (off)           | Mode        | Verdict |
| --------- | ----------------------- | ----------- | ------- |
| Q-C0      | none (control)          | headless    | PASS    |
| Q-S1-1..3 | beta, proj              | headless    | PASS ×3 |
| Q-S2-1..3 | alpha, gamma            | headless    | PASS ×3 |
| Q-H1      | beta, proj + user hooks | headless    | PASS    |
| Q-S1-I    | beta, proj              | interactive | PASS    |

The limits of the 2.1.314 record apply unchanged.
