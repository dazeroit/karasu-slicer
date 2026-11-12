# M4 validation and acceptance

## Scope and provenance

Validation on 2026-10-17 covers the independently designed [LCD contract](M4-LCD-IMPLEMENTATION.md)
and [PNG framing](M4-PNG-DESIGN.md). Parent HEAD is `2e610a1`; this M4 working-tree increment is
uncommitted. Research records keep observations, independent choices and unknowns separately. No
commercial runtime image parity is
asserted; M1 EXP-05/06 orientation/sampling/gray/compensation experiments remain open.

Raw evidence is outside Git at `/tmp/karasu-m4-validation/` and `/tmp/karasu-m4-research/`.
Temporary storage is not durable archival custody. No proprietary code/assets, reference source,
foreign code, new dependency, license selection, installed-application modification, commit or remote
change is part of this implementation.

## Linux/WSL quality gates

Stable native Rust, `x86_64-unknown-linux-gnu`, WSL2 on the existing Ryzen 7 7700 host:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | **645 passed, 0 failed, 2 ignored** |
| `cargo build --workspace` | PASS |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items` | PASS |
| `git diff --check` | PASS |

This adds 33 passing tests to the renamed baseline. The coverage includes exhaustive supported
quantizer domains, all orientation combinations, analytic masks/holes/islands and pixel boundaries,
independent point and grayscale-neighborhood oracles, strict and constructed-rational material,
large-origin precision rejection, clip/reject policy, all caps, cancellation, writer errors and fused
stream behavior. Full owned-fixture replay includes generated FX11 (327,680 source triangles),
complete-layer deterministic rerendering and explicit unresolved-layer rejection. The two prior
optional file/measurement tests remain ignored; FX11 is exercised separately in memory.

## Independent PNG verification

The Python standard-library validator independently parses PNG framing/CRC, decompresses and validates
zlib/Adler through `zlib`, rejects trailing/unconsumed data, checks filter/lengths and compares every
pixel with a separate physical-space point oracle. Both 80x48 and 512x320 at N=4 pass, including
multiple stored blocks. The 512x320 intensity sum is 14,365,012; pixel SHA-256 is
`6da719f94e4f3b691867ae0e27c551b098e9c768cfd030eaedd473ba50a7201c`; PNG SHA-256 is
`ba016aa5bde8334937a37b63f4389739bdf6bd8e1196970214b1ac6a0d916aa1`.
The 80x48 golden PNG hash is `7443de9b5eea0952109ecccce277ae1e574d3bdb87923a73cc9d1b89e70888d7`.
Large benchmark PNGs are separately decoded and their dimensions/nonzero counts/intensity sums
compared with Rust-reported metadata. Repeat outputs match bytes; matching hashes alone are not a
correctness oracle.

## Preliminary performance and memory

Budgets declared before measurements: raster-only <=5 seconds, planned requested payload <=256 MiB,
whole-process peak RSS <=512 MiB. Each row below is three fresh Release example processes, without
CPU affinity, cache flushing, allocator instrumentation or subtraction of overhead. Timings include
raster validation/conversion/allocation/render/statistics; M3 and PNG times are separate. Host/WSL
scheduling and other activity are uncontrolled. Bounds apply to these workloads, not arbitrary jobs.

| Workload | Raster min / median / max | Planned bytes | Process peak RSS |
| --- | --- | --- | --- |
| 7680x4320, N4, triangle + hole | 367.649 / 367.797 / 407.359 ms | 33,193,744 | 35,328 KiB in all three |
| 3840x2160, N4, 1,000-edge ellipse | 122.327 / 122.629 / 134.857 ms | 8,414,080 | 12,844..12,968 KiB |

The 1000-edge example initially encountered M3's default validity pair cap before rasterization.
The profiling example now explicitly permits 4,000,000 upstream validity pairs (2,000 section edges
need 1,999,000 pairs); production defaults are unchanged. M3 section times are approximately
37..41 ms for this case. Raster/PNG timings do not conceal that work.

A separately compiled temporary copy of the same public example changes only compensation to
`Dilate([512,512])`: one 7680x4320 N4 observation renders in 831.867 ms, plans 66,432,784 bytes and
peaks at 67,712 KiB RSS. This is a bounded additional compensation observation, not a three-run
baseline or a comparison against an alternative algorithm. Its source and output are retained
outside Git. Large radii use the same linear-time queue algorithm as correctness-tested small cases.

Uncompensated large PNG hashes (identical across all three runs):

- LCD: `2b07b00cd73032aafa893440e5beba8dc7872d6626abfe1192fc6edd388664fc`.
- Ellipse: `56c5b599c6125d29a7988c623927121ee633cca6b1e3c50b7589308e83d6c96b`.

These observations satisfy all preliminary budgets. Active-edge traversal, row counts instead of an
upscaled image, separable extrema and bounded PNG blocks reduce unnecessary work/storage by design;
no measured speedup relative to commercial applications or copied source is claimed.

## Native Windows

Native validation uses a separate 169-file snapshot with before/after SHA-256 checks. Source ZIP
SHA-256: `ac0296cf206e11e4b0e52c6922c6a42d7eba2a3ce2b5b81a9d4871b24321ca40`.
Production Rust/Cargo/test/example files in the final tree are checked against that snapshot;
status documentation added afterward is not misrepresented as having been part of the export.

All 105 Rust/Python/Cargo/test/example/tool files in the final tree match the exported source hashes.
Before/after native snapshot integrity checks pass for all 169 files. Native MSVC results:

| Check | Result |
| --- | --- |
| Full workspace Debug suite | **644 passed, 0 failed, 2 ignored** |
| Full workspace Release suite, unchanged complete retry | **644 passed, 0 failed, 2 ignored** |
| Three focused M4 Release integration suites + M4 unit tests | **33 passed, 0 failed** |
| Format, strict all-target Clippy, workspace build, warnings-denied public/private rustdoc | PASS |
| Reference example Release build and all three PNG export runs | PASS |

The single-test count difference from Linux is the existing Unix-only path regression. Unlike
previous revisions, this snapshot completes the full native Debug suite. The first full Release
attempt ran 643 passing tests and encountered Windows application-control OS error 4551 while
launching the preexisting M3 `slicing::paths` doctest. Its unchanged isolated retry passed. The
subsequent unchanged **complete workspace Release retry passed 644/0/2**, including all doctests.
Initial failure logs remain retained; they are not relabeled as successes. The later passes establish
functional acceptance of this snapshot, not a permanent host-policy repair. No binary alteration or
application-control policy change was used to make tests execute.

All three Windows PNG hashes equal the Linux hashes, including the independent-oracle 512x320
reference. Native one-run raster observations: 512x320 N4 1.692 ms; 7680x4320 N4 340.323 ms;
3840x2160 N4 1000-edge ellipse 118.360 ms. These are native observations, not repeated timing/RSS
baselines. The complete Debug and Release suites establish native functional acceptance
for M4's documented contracts. Future host launch failures must still be recorded explicitly.

## Remaining boundaries

The supported raster arithmetic domain, AA subcell feature loss, lack of optical calibration,
rectangular rather than Euclidean compensation, and caller-owned output retention are explicit
contracts. Stored DEFLATE produces larger files than compressed PNG. Arbitrary writer failure may
leave partial bytes; job/file transaction orchestration belongs to a later adapter. No GUI preview
widget, printer placement, M5 machine format or M5.5 model preparation is implemented here.
