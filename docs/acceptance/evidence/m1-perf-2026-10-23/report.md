### shape-a: 1 terminals, 1 project(s)

Runs: #1 (loadavg 0.36 0.67 1.12, 8520 MiB available, leftovers 0), #2 (loadavg 0.30 0.47 0.85, 8313 MiB available, leftovers 0), #3 (loadavg 0.26 0.50 0.73, 8362 MiB available, leftovers 0)

| latency (ms) | n | failed | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- | --- | --- |
| restore (cold start of the Projects view), run 1 | 3 | 0 | 137 | 150 | 150 | 150 |
| restore (cold start of the Projects view), run 2 | 3 | 0 | 134 | 139 | 139 | 139 |
| restore (cold start of the Projects view), run 3 | 3 | 0 | 135 | 135 | 135 | 135 |
| **restore (cold start of the Projects view), all runs** | 9 | 0 | 135 | 150 | 150 | 150 |
| project switch, run 1 | 39 | 0 | 92 | 111 | 114 | 114 |
| project switch, run 2 | 39 | 0 | 80 | 111 | 128 | 128 |
| project switch, run 3 | 39 | 0 | 79 | 110 | 114 | 114 |
| **project switch, all runs** | 117 | 0 | 80 | 111 | 114 | 128 |
| input to render (screen clear), run 1 | 40 | 0 | 68 | 86 | 87 | 87 |
| input to render (screen clear), run 2 | 40 | 0 | 68 | 83 | 84 | 84 |
| input to render (screen clear), run 3 | 40 | 0 | 70 | 86 | 88 | 88 |
| **input to render (screen clear), all runs** | 120 | 0 | 69 | 84 | 87 | 88 |
| input to render (one character), run 1 | 40 | 0 | 61 | 66 | 67 | 67 |
| input to render (one character), run 2 | 40 | 0 | 62 | 80 | 81 | 81 |
| input to render (one character), run 3 | 40 | 0 | 61 | 66 | 68 | 68 |
| **input to render (one character), all runs** | 120 | 0 | 61 | 68 | 80 | 81 |

| idle state | run | processes | RSS MiB | PSS MiB | threads | fds | CPU % of a core | wake-ups/s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| console_only | 1 | 5 | 762 | 462 | 83 | 140 | 0.67 | 131 |
| console_only | 2 | 5 | 764 | 464 | 83 | 140 | 0.63 | 128 |
| console_only | 3 | 5 | 763 | 463 | 84 | 140 | 0.65 | 132 |
| projects_shown | 1 | 5 | 748 | 436 | 85 | 139 | 0.65 | 103 |
| projects_shown | 2 | 5 | 748 | 436 | 84 | 139 | 0.65 | 107 |
| projects_shown | 3 | 5 | 742 | 430 | 91 | 139 | 0.72 | 144 |
| projects_hidden | 1 | 5 | 812 | 496 | 83 | 136 | 0.40 | 58 |
| projects_hidden | 2 | 5 | 812 | 495 | 83 | 136 | 0.37 | 59 |
| projects_hidden | 3 | 5 | 799 | 483 | 82 | 136 | 0.42 | 80 |

| added by the Projects view while shown (shown minus hidden) | run | PSS MiB | CPU % of a core | wake-ups/s |
| --- | --- | --- | --- | --- |
| | 1 | -60 | +0.25 | +45 |
| | 2 | -59 | +0.28 | +48 |
| | 3 | -53 | +0.30 | +65 |

### shape-b: 16 terminals, 16 project(s)

Runs: #1 (loadavg 0.60 0.60 0.70, 8357 MiB available, leftovers 0), #2 (loadavg 0.25 0.89 0.99, 8663 MiB available, leftovers 0), #3 (loadavg 0.31 0.73 0.91, 8700 MiB available, leftovers 0)

| latency (ms) | n | failed | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- | --- | --- |
| restore (cold start of the Projects view), run 1 | 3 | 0 | 156 | 163 | 163 | 163 |
| restore (cold start of the Projects view), run 2 | 3 | 0 | 149 | 167 | 167 | 167 |
| restore (cold start of the Projects view), run 3 | 3 | 0 | 156 | 156 | 156 | 156 |
| **restore (cold start of the Projects view), all runs** | 9 | 0 | 156 | 167 | 167 | 167 |
| project switch, run 1 | 39 | 0 | 129 | 179 | 498 | 498 |
| project switch, run 2 | 39 | 0 | 129 | 179 | 275 | 275 |
| project switch, run 3 | 39 | 0 | 144 | 216 | 252 | 252 |
| **project switch, all runs** | 117 | 0 | 130 | 179 | 275 | 498 |
| input to render (screen clear), run 1 | 40 | 0 | 69 | 86 | 86 | 86 |
| input to render (screen clear), run 2 | 40 | 0 | 70 | 86 | 87 | 87 |
| input to render (screen clear), run 3 | 40 | 0 | 86 | 104 | 104 | 104 |
| **input to render (screen clear), all runs** | 120 | 0 | 71 | 90 | 104 | 104 |
| input to render (one character), run 1 | 40 | 0 | 62 | 68 | 88 | 88 |
| input to render (one character), run 2 | 40 | 0 | 61 | 76 | 82 | 82 |
| input to render (one character), run 3 | 40 | 0 | 76 | 88 | 96 | 96 |
| **input to render (one character), all runs** | 120 | 0 | 64 | 83 | 88 | 96 |

| idle state | run | processes | RSS MiB | PSS MiB | threads | fds | CPU % of a core | wake-ups/s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| console_only | 1 | 20 | 1023 | 562 | 173 | 368 | 7.06 | 1172 |
| console_only | 2 | 20 | 1016 | 557 | 173 | 368 | 6.28 | 1442 |
| console_only | 3 | 20 | 1019 | 559 | 178 | 367 | 7.28 | 1220 |
| projects_shown | 1 | 20 | 958 | 488 | 176 | 366 | 9.07 | 1460 |
| projects_shown | 2 | 20 | 1046 | 574 | 179 | 366 | 9.23 | 1429 |
| projects_shown | 3 | 20 | 990 | 520 | 177 | 366 | 9.05 | 1459 |
| projects_hidden | 1 | 20 | 1016 | 542 | 175 | 363 | 5.58 | 1351 |
| projects_hidden | 2 | 20 | 1025 | 553 | 178 | 362 | 5.63 | 1336 |
| projects_hidden | 3 | 20 | 1051 | 577 | 176 | 363 | 5.48 | 1336 |

| added by the Projects view while shown (shown minus hidden) | run | PSS MiB | CPU % of a core | wake-ups/s |
| --- | --- | --- | --- | --- |
| | 1 | -54 | +3.48 | +109 |
| | 2 | +21 | +3.60 | +93 |
| | 3 | -57 | +3.57 | +123 |

### shape-c: 16 terminals, 1 project(s)

Runs: #1 (loadavg 0.61 0.91 1.05, 8707 MiB available, leftovers 0), #2 (loadavg 0.82 0.85 0.98, 8838 MiB available, leftovers 0), #3 (loadavg 0.48 0.77 0.93, 8784 MiB available, leftovers 0)

| latency (ms) | n | failed | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- | --- | --- |
| restore (cold start of the Projects view), run 1 | 3 | 0 | 597 | 600 | 600 | 600 |
| restore (cold start of the Projects view), run 2 | 3 | 0 | 589 | 599 | 599 | 599 |
| restore (cold start of the Projects view), run 3 | 3 | 0 | 596 | 633 | 633 | 633 |
| **restore (cold start of the Projects view), all runs** | 9 | 0 | 596 | 633 | 633 | 633 |
| project switch, run 1 | 39 | 0 | 512 | 592 | 649 | 649 |
| project switch, run 2 | 39 | 0 | 523 | 552 | 578 | 578 |
| project switch, run 3 | 39 | 0 | 526 | 592 | 611 | 611 |
| **project switch, all runs** | 117 | 0 | 513 | 578 | 611 | 649 |

| idle state | run | processes | RSS MiB | PSS MiB | threads | fds | CPU % of a core | wake-ups/s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| console_only | 1 | 20 | 1010 | 550 | 175 | 368 | 6.45 | 1457 |
| console_only | 2 | 20 | 1010 | 549 | 175 | 368 | 6.83 | 1367 |
| console_only | 3 | 20 | 1010 | 549 | 175 | 368 | 6.55 | 1436 |
| projects_shown | 1 | 20 | 994 | 525 | 197 | 395 | 6.53 | 1596 |
| projects_shown | 2 | 20 | 1002 | 530 | 196 | 395 | 6.77 | 1601 |
| projects_shown | 3 | 20 | 1016 | 546 | 197 | 395 | 6.47 | 1585 |
| projects_hidden | 1 | 20 | 1039 | 566 | 181 | 363 | 4.92 | 1318 |
| projects_hidden | 2 | 20 | 1044 | 568 | 180 | 363 | 5.10 | 1326 |
| projects_hidden | 3 | 20 | 1037 | 563 | 180 | 363 | 4.95 | 1325 |

| added by the Projects view while shown (shown minus hidden) | run | PSS MiB | CPU % of a core | wake-ups/s |
| --- | --- | --- | --- | --- |
| | 1 | -41 | +1.62 | +277 |
| | 2 | -38 | +1.67 | +275 |
| | 3 | -17 | +1.52 | +260 |

### Proposed reference budgets (not approved)

| shape | restore ms | switch ms | input ms (clear) | input ms (one char) | whole tree PSS MiB (shown) | the view adds, CPU % / wake-ups per second |
| --- | --- | --- | --- | --- | --- | --- |
| shape-a | 250 | 175 | 150 | 125 | 550 | 1.0 / 100 |
| shape-b | 275 | 325 | 175 | 150 | 725 | 7.2 / 200 |
| shape-c | 950 | 900 | - | - | 700 | 3.3 / 425 |
