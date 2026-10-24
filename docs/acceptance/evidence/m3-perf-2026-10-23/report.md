# M3 performance — Claude Managed, offline mock provider

7 runs per profile on `Intel(R) Core(TM) i5-10300H CPU @ 2.50GHz` (16192556 kB), kernel 6.18.55-2-cachyos-lts.
Fixture workload: one short scripted answer; no network, no real provider, no credential.
p99 equals the maximum with this many runs. _Proposed_ = 1.5 × p95 rounded up: a proposal for the
owner, not an enforced budget.

## claude_inference_v1 (n=7)

| figure                                                        | min    | p50    | p95    | max    | proposed   |
| ------------------------------------------------------------- | ------ | ------ | ------ | ------ | ---------- |
| launch to runtime ready (cold: bridge, SDK import, CLI start) | 2210.4 | 2224.2 | 2242.4 | 2242.4 | 3364 ms    |
| bind a conversation                                           | 0.0    | 0.0    | 0.1    | 0.1    | 1 ms       |
| turn start to first streamed text                             | 40.8   | 41.0   | 41.1   | 41.1   | 62 ms      |
| turn start to completed (short answer)                        | 51.2   | 51.4   | 51.7   | 51.7   | 78 ms      |
| stop request to whole tree reaped                             | 502.8  | 503.6  | 524.1  | 524.1  | 787 ms     |
| process-tree PSS when ready                                   | 303229 | 303723 | 304102 | 304102 | 456153 KiB |
| process-tree PSS after a turn                                 | 319275 | 319384 | 320274 | 320274 | 480411 KiB |
| process-tree RSS when ready                                   | 337688 | 338168 | 338580 | 338580 | 507870 KiB |
| processes in the tree                                         | 7      | 7      | 7      | 7      | 11         |
| threads in the tree                                           | 19     | 20     | 20     | 20     | 30         |
| open file descriptors in the tree                             | 47     | 47     | 47     | 47     | 71         |
| CPU of the tree while idle (5 s)                              | 2.8    | 2.8    | 3.0    | 3.0    | 5 %        |
| context switches of the tree while idle                       | 16.9   | 19.9   | 21.5   | 21.5   | 33 /s      |
| processes left after stop                                     | 0      | 0      | 0      | 0      | 0          |

Idle CPU by process (p50 %): `bwrap` 0.0, `claude` 2.8, `python` 0.0, `python3` 0.0

## claude_local_tools_v1 (n=7)

| figure                                                        | min    | p50    | p95    | max    | proposed   |
| ------------------------------------------------------------- | ------ | ------ | ------ | ------ | ---------- |
| launch to runtime ready (cold: bridge, SDK import, CLI start) | 2236.9 | 2246.2 | 2264.3 | 2264.3 | 3397 ms    |
| bind a conversation                                           | 0.0    | 0.0    | 0.0    | 0.0    | 0 ms       |
| turn start to first streamed text                             | 40.9   | 40.9   | 41.2   | 41.2   | 62 ms      |
| turn start to completed (short answer)                        | 51.1   | 51.3   | 51.6   | 51.6   | 78 ms      |
| stop request to whole tree reaped                             | 483.4  | 503.1  | 523.4  | 523.4  | 786 ms     |
| process-tree PSS when ready                                   | 315132 | 315574 | 315826 | 315826 | 473739 KiB |
| process-tree PSS after a turn                                 | 331243 | 331489 | 331918 | 331918 | 497877 KiB |
| process-tree RSS when ready                                   | 349628 | 350016 | 350188 | 350188 | 525282 KiB |
| processes in the tree                                         | 7      | 7      | 7      | 7      | 11         |
| threads in the tree                                           | 19     | 19     | 20     | 20     | 30         |
| open file descriptors in the tree                             | 47     | 47     | 47     | 47     | 71         |
| CPU of the tree while idle (5 s)                              | 3.0    | 3.2    | 3.2    | 3.2    | 5 %        |
| context switches of the tree while idle                       | 20.5   | 22.9   | 24.6   | 24.6   | 37 /s      |
| processes left after stop                                     | 0      | 0      | 0      | 0      | 0          |

Idle CPU by process (p50 %): `bwrap` 0.0, `claude` 3.2, `python` 0.0, `python3` 0.0

## Cancel while streaming (n=7)

request to the interruption notice: min 10.3 · p50 10.3 · p95 10.3 · max 10.3 ms

## Not measured here

- Codex on the same workload: needs a Codex runtime started on this machine, which this pass did not do
  (no provider credential or configuration was read or created). The same harness shape applies; the
  comparison stays PROVIDER_EVIDENCE_REQUIRED.
- Real provider latency, quota and entitlement.
