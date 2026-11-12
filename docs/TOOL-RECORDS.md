# Tool records

Results and history moved from the tools' READMEs on 2026-11-10 (owner): each README says how
to use its tool; what the tools found, and when, is kept here unchanged.

## CLI comparison suite: history

The suite was first kept outside the repository and recreated from the session record in M6.4c
c5 (2026-10-21), when that copy was lost with a WSL restart. Its baseline is the M6.4c c4 build
(`c8399b6`): 507 files.

## M3 measurement protocol: initial Linux/WSL Release observation

Measured 2026-10-17 against the uncommitted M3 constructive extension of parent HEAD
`7ee0440299837b78db8e27f86fe689ed149a906d`. Raw samples, environment data and SHA-256
source/binary manifest are external in `/tmp/resin-m3-profile/`; the measured executable
SHA-256 is `0367f76576c2d0978d330140472b59e40a7989b8a30b67ce23a8373b00682c7a`.
This temporary directory
is not a durable artifact archive. Commands ran sequentially without CPU affinity or
frequency control. No cache flush, allocator instrumentation, baseline subtraction or
concurrent measurement was used. Other host activity and WSL scheduling remain uncontrolled.

Environment: stable rustc 1.99.0 (`b940084d7`, LLVM 23.1.1),
`x86_64-unknown-linux-gnu`; WSL2 Linux `6.6.87.2-microsoft-standard-WSL2`;
AMD Ryzen 7 7700, eight cores / sixteen logical CPUs.

| Case | Batch repetitions | Per-operation min / median / max | Retained output bytes | Three peak-RSS observations, KiB |
| --- | ---: | --- | ---: | --- |
| `strict-nested` | 4 | 2.058 / 2.074 / 2.079 ms | 128,208 | 2688 / 2688 / 2688 |
| `strict-many` | 1 | 15.683 / 15.725 / 15.769 ms | 886,080 | 3712 / 3712 / 3712 |
| `face-heavy` | 1 | 6.390 / 6.435 / 6.504 ms | 18,192 | 3072 / 3072 / 3072 |
| `construct-overlap` | 1 | 13.358 / 13.395 / 13.578 ms | 188,488 | 2688 / 2816 / 2688 |
| `construct-rational` | 32 | 36.425 / 37.027 / 41.738 microseconds | 3,208 | 2560 / 2432 / 2560 |
| `pair-cap` | 32 | Timer/loop overhead dominates; no useful speed interpretation | 0 | 2560 / 2432 / 2560 |

Strict times are for complete three-layer collections, not single layers. Memory mode
includes fixture creation, source mesh, working buffers, owned output, runtime and
validation. In particular, `pair-cap` RSS includes the successful 15-pair validation
operation, despite the rejected operation having zero retained output. Requested output
capacity and RSS measure different things and cannot be subtracted to attribute allocations.

All reported runs passed analytic validation. These bounded small/medium workloads establish
a reproducible observation baseline; they do not accept large-input memory goals. Selecting
owner-approved performance/memory budgets needs larger representative corpus measurements,
and native Windows must be measured separately. No production optimization follows solely
from this baseline.

## M3 measurement protocol: TD-031 represented-coordinate results

On 2026-10-17 the Linux Release worker passes all validations. Ten post-warmup batches give median
5.598 ms per seven-layer nested operation (299,264 retained payload bytes) and 34.055 ms per
seven-layer 64-region operation (2,067,632 bytes). One separate fresh-process many-region memory
run reports 4,864 KiB maximum RSS. Raw records are `/tmp/resin-m3-represented-{nested,many}.csv`,
`/tmp/resin-m3-represented-many-memory.txt` and `/tmp/resin-m3-represented-many-rss.txt`.
These are preliminary observations, not a speedup comparison, repeated memory campaign, Windows
measurement or adopted performance budget. No upstream implementation was benchmarked.

## Native Windows validation: observed M5 revalidation

On 2026-10-17, this procedure passed the full native Debug suite (717/0/2), fmt, clippy, workspace
build and public/private rustdoc. Clean Release compilation was rejected by
`VerifiedAndReputableDesktop` before a `proc-macro2` build script ran (4551). The original M5 export
test was independently rejected by the same policy. Consequently this documents a repeatable
validation and diagnosis procedure, not a method that makes the current host's Release gate pass.
No code, toolchain options or security policy was changed to evade enforcement. See the M5 report
for exact hashes, preserved failures and the prerequisites for closing acceptance.

## Support test plate: the printed plates

**The presets changed after these prints** (TD-136, 2026-11-06). The documents name a preset and
override only some values, so karasu resolves the rest from today's presets: generated now, a
plate is not the one printed (Medium and Heavy are shallower, Heavy's tip is 1.0 mm and its
shaft 1.2 mm). The printed plates were generated and sliced at commits 3ea0b80 (first) and
1cc1aab (second).
