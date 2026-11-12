# M2.4-C mesh performance and memory baseline

## Provenance and method

Initial baseline acceptance: **PASS**, measured 2026-10-15 on `main`, production commit
`21199691ed6d15b9d25f19e60df3869c2829a36f` (committed M2.4-B). The starting tree was clean.
Only measurement infrastructure and documentation changed; no production geometry, representation,
dependencies, fixture generator, canonical fixtures, or M1 research changed. No commit was created.
The worker and runner are new uncommitted infrastructure: their exact copies/hashes are archived
separately rather than falsely attributed to the production commit.

External session:
`<evidence>/m2/m2-4c/20261015-2119969-t3MuPY/`.
It contains provenance/fixture/executable JSON, exact commands and exit statuses, stdout/stderr,
raw batch CSV, timing/memory JSON, validation records, 99 GNU Time reports, and an integrity manifest
excluding build intermediates. Executable SHA-256:
`12064f5b698ebff00c6e7c1206822a6edb66337a34088dbf027c68cffd9794f2`.

AMD Ryzen 7 7700 (8 cores/16 logical CPUs), WSL2 kernel
`6.6.87.2-microsoft-standard-WSL2`, `x86_64-unknown-linux-gnu`. WSL exposed 16,320,233,472 bytes RAM
(15.20 GiB), with 13,871,087,616 bytes available before execution; 4 GiB swap unused. Load averages
before/after were 0.00/0.03/0.17 and 0.23/0.08/0.19. Windows-host interference is not excluded.
Stable Rust 1.99.0 (`b940084d7`, LLVM 23.1.1), Cargo 1.99.0 (`5f94df478`). Captured verbose compilation
confirms default Cargo Release, `-C opt-level=3`, stripped debuginfo, no LTO/custom flags/instrumentation.
No software or dependency was installed. Compilation preceded measurements. This is Linux/WSL
performance evidence, not native Windows performance evidence.

See [the harness protocol](../tools/mesh-profile/README.md) for reproduction and precise scopes.
Every workload has three warm-up batches and ten timed batches. Small read/import/topology batches
contain 32 operations; small bounds batches 8,192; FX-11 batches one. Black-boxed results are retained
in a preallocated vector until timing stops: validation/destruction are excluded, but loop, Result
branching, barriers, and retention pushes are included. Batch averages are not individual-call tails.
All samples are preserved, including outliers. Processes are serial with 60-second watchdogs (180
seconds for build). No CPU pinning, frequency isolation, cold-cache control, or allocator tracing.

File reading is `std::fs::read`; import uses resident bytes, default Auto detection, and explicit assumed
millimeters. Import includes exact-coordinate deduplication, source bounds, metadata, mesh validation,
and geometry/metadata diagnostics: not a pure parser. Topology constructs fresh reports on an imported
resident immutable mesh, including its own basic diagnostics, with no timed read/import. World bounds
visits every actual vertex, not eight source-AABB corners. Python independently decodes binary STL
corners and uses scalar Rx -> Ry -> Rz equations for expected extrema. Every result validates outside
timing; endpoint tolerance is 256*f64::EPSILON*100 mm for these sub-100-mm coordinates, dimensions
twice that. This is not a production epsilon. See protocol for fixed translation, scale, rotation,
and non-origin pivot parameters.

## Fixture and correctness results

Manifest SHA-256 and sizes match for all four STLs, including existing ignored FX-11; none regenerated.
Full paths/hashes and independent transformed extrema are in external `fixtures.json`.

| Fixture | Bytes | Source V | Source/usable F | E | Boundary / non-manifold / winding | Edge / vertex components |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| FX-01 | 684 | 8 | 12 / 12 | 18 | 0 / 0 / 0 | 1 / 1 |
| FX-02 | 25,684 | 258 | 512 / 512 | 768 | 0 / 0 / 0 | 1 / 1 |
| FX-05 | 684 | 8 | 12 / 12 | 18 | 0 / 0 / 0 | 1 / 1 |
| FX-11 | 16,384,084 | 163,842 | 327,680 / 327,680 | 491,520 | 0 / 0 / 0 | 1 / 1 |

Every vertex has an interior cycle link. Exclusions, duplicates, isolated/indeterminate/non-manifold
vertices, import diagnostics, and topology issues are zero. Edge/vertex-manifold, supplied local
winding, and closed-combinatorial checks pass; this is not self-intersection/outward-volume/printability
validation. Facet metadata counts and independent world-bound endpoints/dimensions also pass.

## Read and import measurements

All times below are **milliseconds per operation**, across ten batch means. Spread is inclusive-
quartile IQR, not a confidence interval. MiB/s divides encoded input size by median time.
Read throughput reflects warm page-cache behavior, not storage-device bandwidth.

| Fixture | Operation | Median ms | Min ms | Max ms | IQR ms | Median MiB/s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| FX-01 | Read | 0.004763 | 0.004645 | 0.006123 | 0.000788 | 136.957 |
| FX-01 | Import | 0.002855 | 0.002808 | 0.004125 | 0.000324 | 228.442 |
| FX-02 | Read | 0.015192 | 0.014589 | 0.016611 | 0.001882 | 1612.277 |
| FX-02 | Import | 0.130555 | 0.129006 | 0.132683 | 0.001224 | 187.616 |
| FX-05 | Read | 0.004702 | 0.004682 | 0.007266 | 0.000267 | 138.743 |
| FX-05 | Import | 0.003177 | 0.002806 | 0.003800 | 0.000727 | 205.323 |
| FX-11 | Read | 1.059607 | 0.957226 | 1.499665 | 0.192677 | 14746.109 |
| FX-11 | Import | 157.718172 | 154.511181 | 176.394328 | 4.742812 | 99.070 |

## Topology measurements

| Fixture | Median ms | Min ms | Max ms | IQR ms |
| --- | ---: | ---: | ---: | ---: |
| FX-01 | 0.004423 | 0.004294 | 0.006466 | 0.000094 |
| FX-02 | 0.181364 | 0.175501 | 0.191501 | 0.006067 |
| FX-05 | 0.005471 | 0.005345 | 0.007501 | 0.000131 |
| FX-11 | 244.856705 | 240.852376 | 258.630491 | 6.395341 |

## Exact transformed-vertex world bounds

All five transforms pass independent extrema/dimension validation on all four fixtures.

| Fixture | Transform | Median ms | Min ms | Max ms | IQR ms |
| --- | --- | ---: | ---: | ---: | ---: |
| FX-01 | Identity | 0.000092 | 0.000090 | 0.000104 | 0.000002 |
| FX-01 | Translation | 0.000092 | 0.000091 | 0.000097 | 0.000003 |
| FX-01 | Scale | 0.000174 | 0.000168 | 0.000286 | 0.000017 |
| FX-01 | Rotation | 0.000167 | 0.000163 | 0.000250 | 0.000005 |
| FX-01 | Combined | 0.000171 | 0.000167 | 0.000200 | 0.000008 |
| FX-02 | Identity | 0.002608 | 0.002586 | 0.002632 | 0.000017 |
| FX-02 | Translation | 0.002629 | 0.002569 | 0.003317 | 0.000032 |
| FX-02 | Scale | 0.005076 | 0.005066 | 0.005172 | 0.000017 |
| FX-02 | Rotation | 0.004891 | 0.004879 | 0.004924 | 0.000016 |
| FX-02 | Combined | 0.004995 | 0.004920 | 0.006318 | 0.000208 |
| FX-05 | Identity | 0.000092 | 0.000089 | 0.000123 | 0.000004 |
| FX-05 | Translation | 0.000090 | 0.000090 | 0.000092 | 0.000001 |
| FX-05 | Scale | 0.000171 | 0.000169 | 0.000232 | 0.000022 |
| FX-05 | Rotation | 0.000166 | 0.000163 | 0.000172 | 0.000005 |
| FX-05 | Combined | 0.000170 | 0.000165 | 0.000175 | 0.000004 |
| FX-11 | Identity | 1.657988 | 1.643441 | 1.701661 | 0.022276 |
| FX-11 | Translation | 1.654397 | 1.613873 | 1.701784 | 0.019091 |
| FX-11 | Scale | 3.266239 | 3.225796 | 3.877167 | 0.013662 |
| FX-11 | Rotation | 3.096309 | 3.083173 | 3.121840 | 0.021439 |
| FX-11 | Combined | 3.120777 | 3.101177 | 3.144184 | 0.027865 |

## Separate process peak memory

GNU `/usr/bin/time -v`, three fresh processes/workload. Entries are median [minimum, maximum]
**KiB** of whole-process peak RSS, not live mesh/heap allocation. Empty-worker baseline:
**2560 [2560,2688] KiB**. Reads retain bytes; imports retain bytes and imported objects. Topology/
bounds import during setup and drop encoded bytes before the operation, but prior allocation peaks
and allocator retention still count. No baseline subtraction or exact phase-increment claim is made.

| Workload | FX-01 KiB | FX-02 KiB | FX-05 KiB | FX-11 KiB |
| --- | --- | --- | --- | --- |
| Read | 2560 [2560,2688] | 2560 [2560,2688] | 2560 [2560,2560] | 18432 [18432,18432] |
| Import | 2688 [2560,2688] | 2816 [2688,2816] | 2688 [2560,2688] | 78220 [78196,78228] |
| Topology | 2688 [2688,2688] | 2816 [2688,2816] | 2688 [2560,2688] | 115600 [115540,115600] |
| Bounds identity | 2688 [2688,2816] | 2944 [2816,2944] | 2816 [2816,2816] | 78224 [78116,78228] |
| Bounds translation | 2688 [2688,2816] | 2944 [2944,2944] | 2816 [2688,2816] | 78224 [78224,78224] |
| Bounds scale | 2816 [2688,2816] | 2944 [2816,2944] | 2816 [2688,2816] | 78228 [78224,78228] |
| Bounds rotation | 2816 [2816,2816] | 2944 [2944,2944] | 2816 [2816,2816] | 78224 [78224,78228] |
| Bounds combined | 2816 [2816,2816] | 2944 [2944,2944] | 2816 [2816,2816] | 78228 [78100,78228] |

Target payload estimates (not RSS): 24 bytes/Point3, 12 bytes/triangle, 32 bytes/facet metadata.
FX-11 used vertex/face/metadata payload totals approximately 17.50005 MiB; directed uses and edge
records add 22.5 and 15 MiB. These exclude capacities, other report data, scratch, input, allocator,
and runtime. They are not measured heap totals or explanations of an exact RSS difference.
Small-process RSS has visibly coarse steps. Three samples do not establish worst-case memory.
Precise allocator attribution was unavailable and no instrumentation was installed.

## Interpretation, validation, and remaining work

This establishes the first repeated baseline, not an optimization result. FX-11 topology takes more
time than import and its process peaks at approximately 112.9 MiB versus import's 76.4 MiB. Should
a future budget motivate optimization, attribute topology sorting/incidence/link work and import
deduplication/diagnostic allocations first; these measurements identify no internal bottleneck.
Bounds timings are consistent with full-vertex scans and the existing identity-linear shortcut,
but four fixtures do not establish an empirical complexity law. Equal-size cube/prism cases also
show that counts alone do not predict timing.

Warm caches, batch retention, tiny-operation worker overhead, fixed workload order, unpinned WSL
frequency/scheduling, and shared-host activity limit generalization. FX-11 import maximum
176.394328 ms versus median 157.718172 ms remains in the data. IQR is not a confidence interval.
Cold I/O, ASCII performance, defective/high-valence worst cases, 32-bit/native Windows performance,
cross-host reproducibility, stack CPU profiles, and precise allocator accounting remain unmeasured.
Historical uncontrolled FX-11 observations (approximately 157 ms M2.1 import, 177.817 ms M2.3 import,
273.917 ms M2.3 topology) differ in scope/cache/load/control. No regression/improvement percentage
against those single observations is justified or calculated.

All 32 timing workloads (320 timed batches) and 33 memory workloads (99 runs) succeeded. Five Python
helper tests pass. Three deliberate worker-negative checks reject wrong vertex count, topology count,
and transformed extrema with exit 1; all real workloads exit 0. An initial Clippy ref-option warning
and an exact-floating-point helper assertion were corrected only in new infrastructure; the external
development log preserves their statuses. No production defect, panic, timeout, or resource failure
was discovered. Stable format, strict Clippy, workspace tests/build, warnings-denied public/private
rustdoc, and `git diff --check` pass. Ordinary results remain **156 passed, 0 failed, 2 ignored**.
Optional historical timing tests remain ignored; controlled FX-11 coverage is provided here instead.
Fixture hashes and production-source integrity are rechecked after validation.

At the time it was recorded, M2.4-C acceptance did not close M2: cancellation semantics and the
roadmap's CLI-inspection deliverable still required review. Closures A–C and final native Windows
functional validation subsequently passed at production baseline
`d8816303bbc4c2ba26213645ddf278db6574342b`; the performance campaign was not rerun and remains tied
to its recorded commit and WSL/Linux environment. No optimization or new feature is authorized by
this baseline, and it supplies no native Windows performance claim.
