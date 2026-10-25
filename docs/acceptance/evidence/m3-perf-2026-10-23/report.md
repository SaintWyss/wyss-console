# M3 performance — Claude Managed, offline mock provider

7 runs per profile on `Intel(R) Core(TM) i5-10300H CPU @ 2.50GHz` (16192556 kB), kernel 6.18.55-2-cachyos-lts.
Fixture workload: one short scripted answer; no network, no real provider, no credential.
p99 equals the maximum with this many runs. _Proposed_ = 1.5 × p95 rounded up: a proposal for the
owner, not an enforced budget.

## claude_inference_v1 (n=7)

| figure                                                        | min    | p50    | p95    | max    | proposed   |
| ------------------------------------------------------------- | ------ | ------ | ------ | ------ | ---------- |
| launch to runtime ready (cold: bridge, SDK import, CLI start) | 2218.8 | 2229.6 | 2242.5 | 2242.5 | 3364 ms    |
| bind a conversation                                           | 0.0    | 0.0    | 0.1    | 0.1    | 1 ms       |
| turn start to first streamed text                             | 40.9   | 41.0   | 41.1   | 41.1   | 62 ms      |
| turn start to completed (short answer)                        | 51.2   | 51.3   | 51.5   | 51.5   | 78 ms      |
| stop request to whole tree reaped                             | 483.1  | 502.5  | 523.0  | 523.0  | 785 ms     |
| process-tree PSS when ready                                   | 303512 | 304123 | 304585 | 304585 | 456878 KiB |
| process-tree PSS after a turn                                 | 319463 | 319672 | 319957 | 319957 | 479936 KiB |
| process-tree RSS when ready                                   | 337500 | 338156 | 338504 | 338504 | 507756 KiB |
| processes in the tree                                         | 7      | 7      | 7      | 7      | 11         |
| threads in the tree                                           | 19     | 19     | 20     | 20     | 30         |
| open file descriptors in the tree                             | 47     | 47     | 47     | 47     | 71         |
| CPU of the tree while idle (5 s)                              | 2.6    | 2.8    | 3.0    | 3.0    | 5 %        |
| context switches of the tree while idle                       | 18.7   | 19.3   | 20.5   | 20.5   | 31 /s      |
| processes left after stop                                     | 0      | 0      | 0      | 0      | 0          |

Idle CPU by process (p50 %): `bwrap` 0.0, `claude` 2.8, `python` 0.0, `python3` 0.0

## claude_local_tools_v1 (n=7)

| figure                                                        | min    | p50    | p95    | max    | proposed   |
| ------------------------------------------------------------- | ------ | ------ | ------ | ------ | ---------- |
| launch to runtime ready (cold: bridge, SDK import, CLI start) | 2243.9 | 2252.8 | 2432.7 | 2432.7 | 3650 ms    |
| bind a conversation                                           | 0.0    | 0.0    | 0.0    | 0.0    | 0 ms       |
| turn start to first streamed text                             | 40.9   | 40.9   | 41.0   | 41.0   | 62 ms      |
| turn start to completed (short answer)                        | 51.2   | 51.2   | 51.5   | 51.5   | 78 ms      |
| stop request to whole tree reaped                             | 482.8  | 503.1  | 504.2  | 504.2  | 757 ms     |
| process-tree PSS when ready                                   | 315996 | 316141 | 316640 | 316640 | 474960 KiB |
| process-tree PSS after a turn                                 | 331848 | 331970 | 332485 | 332485 | 498728 KiB |
| process-tree RSS when ready                                   | 349884 | 350180 | 350608 | 350608 | 525912 KiB |
| processes in the tree                                         | 7      | 7      | 7      | 7      | 11         |
| threads in the tree                                           | 19     | 20     | 20     | 20     | 30         |
| open file descriptors in the tree                             | 47     | 47     | 47     | 47     | 71         |
| CPU of the tree while idle (5 s)                              | 3.0    | 3.2    | 3.2    | 3.2    | 5 %        |
| context switches of the tree while idle                       | 22.6   | 23.8   | 26.8   | 26.8   | 41 /s      |
| processes left after stop                                     | 0      | 0      | 0      | 0      | 0          |

Idle CPU by process (p50 %): `bwrap` 0.0, `claude` 3.2, `python` 0.0, `python3` 0.0

## Cancel while streaming (n=7)

request to the interruption notice: min 10.3 · p50 10.3 · p95 10.6 · max 10.6 ms

## Not measured here

- Codex on the same workload: needs a Codex runtime started on this machine, which this pass did not do
  (no provider credential or configuration was read or created). The same harness shape applies; the
  comparison stays PROVIDER_EVIDENCE_REQUIRED.
- Real provider latency, quota and entitlement.
