# M3 performance — Claude Managed, offline mock provider

7 runs per profile on `Intel(R) Core(TM) i5-10300H CPU @ 2.50GHz` (16192556 kB), kernel 6.18.55-2-cachyos-lts.
Fixture workload: one short scripted answer; no network, no real provider, no credential.
p99 equals the maximum with this many runs. _Proposed_ = 1.5 × p95 rounded up: a proposal for the
owner, not an enforced budget.

## claude_inference_v1 (n=7)

| figure                                                        | min    | p50    | p95    | max    | proposed   |
| ------------------------------------------------------------- | ------ | ------ | ------ | ------ | ---------- |
| launch to runtime ready (cold: bridge, SDK import, CLI start) | 2215.6 | 2227.2 | 2249.2 | 2249.2 | 3374 ms    |
| bind a conversation                                           | 0.0    | 0.0    | 0.0    | 0.0    | 0 ms       |
| turn start to first streamed text                             | 40.9   | 41.0   | 41.1   | 41.1   | 62 ms      |
| turn start to completed (short answer)                        | 51.2   | 51.4   | 51.4   | 51.4   | 78 ms      |
| stop request to whole tree reaped                             | 502.8  | 503.3  | 522.8  | 522.8  | 785 ms     |
| process-tree PSS when ready                                   | 289807 | 290408 | 290854 | 290854 | 436281 KiB |
| process-tree PSS after a turn                                 | 305447 | 305908 | 307107 | 307107 | 460661 KiB |
| process-tree RSS when ready                                   | 307796 | 308412 | 308816 | 308816 | 463224 KiB |
| processes in the tree                                         | 4      | 4      | 4      | 4      | 6          |
| threads in the tree                                           | 15     | 15     | 16     | 16     | 24         |
| open file descriptors in the tree                             | 34     | 34     | 34     | 34     | 51         |
| CPU of the tree while idle (5 s)                              | 2.8    | 3.0    | 3.2    | 3.2    | 5 %        |
| context switches of the tree while idle                       | 18.5   | 20.7   | 22.7   | 22.7   | 35 /s      |
| processes left after stop                                     | 0      | 0      | 0      | 0      | 0          |

Idle CPU by process (p50 %): `bwrap` 0.0, `claude` 3.0, `python` 0.0

## claude_local_tools_v1 (n=7)

| figure                                                        | min    | p50    | p95    | max    | proposed   |
| ------------------------------------------------------------- | ------ | ------ | ------ | ------ | ---------- |
| launch to runtime ready (cold: bridge, SDK import, CLI start) | 2236.6 | 2255.9 | 2269.4 | 2269.4 | 3405 ms    |
| bind a conversation                                           | 0.0    | 0.0    | 0.1    | 0.1    | 1 ms       |
| turn start to first streamed text                             | 40.9   | 41.0   | 41.1   | 41.1   | 62 ms      |
| turn start to completed (short answer)                        | 51.2   | 51.3   | 51.4   | 51.4   | 78 ms      |
| stop request to whole tree reaped                             | 502.1  | 503.3  | 521.7  | 521.7  | 783 ms     |
| process-tree PSS when ready                                   | 301879 | 302261 | 302534 | 302534 | 453801 KiB |
| process-tree PSS after a turn                                 | 317771 | 318530 | 318632 | 318632 | 477948 KiB |
| process-tree RSS when ready                                   | 319960 | 320208 | 320548 | 320548 | 480822 KiB |
| processes in the tree                                         | 4      | 4      | 4      | 4      | 6          |
| threads in the tree                                           | 15     | 16     | 16     | 16     | 24         |
| open file descriptors in the tree                             | 34     | 34     | 34     | 34     | 51         |
| CPU of the tree while idle (5 s)                              | 3.0    | 3.2    | 3.2    | 3.2    | 5 %        |
| context switches of the tree while idle                       | 20.7   | 22.5   | 25.8   | 25.8   | 39 /s      |
| processes left after stop                                     | 0      | 0      | 0      | 0      | 0          |

Idle CPU by process (p50 %): `bwrap` 0.0, `claude` 3.2, `python` 0.0

## Cancel while streaming (n=7)

request to the interruption notice: min 10.3 · p50 10.4 · p95 10.6 · max 10.6 ms

## Not measured here

- Codex on the same workload: needs a Codex runtime started on this machine, which this pass did not do
  (no provider credential or configuration was read or created). The same harness shape applies; the
  comparison stays PROVIDER_EVIDENCE_REQUIRED.
- Real provider latency, quota and entitlement.
