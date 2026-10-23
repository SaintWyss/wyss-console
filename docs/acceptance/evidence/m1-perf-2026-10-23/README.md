# M1-13 measurements on an isolated native display (2026-10-23)

What is here and how to check it. The method is in the delivery record
(`../../m1-delivery-2026-10-23.md`, "Measurements") and in `benchmarks/README.md`.

| File or folder                  | What it is                                                                                                                                                  |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `shape-{a,b,c}/run{1,2,3}.json` | One run each: every latency sample (nothing dropped), the idle summaries of three states, the conditions (load, memory), the leftovers found afterwards      |
| `postreview/shape-*/run1.json`  | One run of each shape on the final build, after the independent review's fixes                                                                              |
| `report.md`                     | The aggregation of the nine runs: `python3 -I benchmarks/m1_gui_report.py docs/acceptance/evidence/m1-perf-2026-10-23` reproduces it byte for byte         |
| `raw-idle-samples.tar.xz`       | The one-second samples of every process of every run (per thread voluntary context switches, memory, processor time), 39 files; SHA-256 `f3cf57545a52b650b1644b38a38cd11799bf64cf15e19dd33279cc7be8f57edb`                |
| `diagnostics/`                  | A 20 s per-thread reading of the application and its web process on shape b with the view shown and hidden                                                  |
| `superseded/`                   | The first attempt (an instrument that summed per-thread counters, which can go backwards); latencies agree with the nine runs                               |

The files were stripped of the raw samples and of the home directory of the machine that made them;
nothing else was changed. Builds: nine runs on commit `9501c4b` (release binary SHA-256
`fdb034250f7cd8598a1af9f36bd2182481d30e5dda18e592cb019c35dd4f7ae4`), the post-review runs on `30756bc`
(`7814ba9c4b00eb190432b9ef00878a1e9d219dce2e2fa6a970e8fec5ff795139`). To read the samples:
`tar -xJf raw-idle-samples.tar.xz -C <empty directory>`; check the archive with `sha256sum` first.
