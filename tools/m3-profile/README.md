# Reproducible M3 Measurement Protocol

The dependency-free worker is `crates/karasu-core/examples/m3_profile.rs`. It measures the
reviewed strict layer pipeline and explicit constructive material composition separately.
These preliminary measurements do not establish worst-case memory, a universal throughput
target, native Windows acceptance, or a reason to add spatial acceleration.

## Running the worker

```console
cargo build --release -p karasu-core --example m3_profile
target/release/examples/m3_profile timing strict-nested 4
target/release/examples/m3_profile timing strict-many 1
target/release/examples/m3_profile timing face-heavy 1
target/release/examples/m3_profile timing construct-overlap 1
target/release/examples/m3_profile timing construct-rational 32
target/release/examples/m3_profile timing pair-cap 32
/usr/bin/time -v target/release/examples/m3_profile memory strict-many 1
```

The three arguments are `timing|memory`, case name and repetitions per timed batch
(1 through 32). Memory mode performs one operation; its repetitions argument is parsed
but ignored. Repeat each memory command in three fresh processes. GNU time reports
whole-process maximum resident set size in KiB.

Timing uses `Instant`, three warmup batches and ten measured batches. Fixture creation,
the explicit three-plane schedule, preallocated batch-result storage, independent result
checks and result destruction are outside each measured interval. Every measured interval
includes the operation, `black_box`, error branching and appending each owned result to the
batch storage. Constructive operations additionally include their small borrowed-ring mapping
allocation. Inputs remain resident; no disk I/O or cold-cache claim is involved. The worker
validates once before timing and after every batch. It exits unsuccessfully on a mismatch.
The case's CSV payload column is retained output element capacity, not process memory or
the sum of all live working-stage buffers.

## Owned analytic workloads

All strict workloads use an explicit origin `[0,0]`, binary grid step `2^-3` mm and the
three planes `-0.5`, `0`, `0.5` mm. Every ribbon spans Z=-1 through Z=1; each four-sided
ring has eight source triangles and eight retained intersection events at every plane.
These coordinates select exact grid-compatible represented intersections. Arbitrary other
plane heights can be rejected by the existing ExactOnly contract when the approximate
triangle-plane construction produces off-grid represented coordinates; this baseline does
not broaden that domain or infer a physical quantization error budget.

| Case | Independently specified geometry and checks |
| --- | --- |
| `strict-nested` | Eight concentric squares with side lengths 64,60,...,36 mm, plus a disjoint 4 mm square: nine boundaries, five material regions and four holes per plane; 72 source events per plane. |
| `strict-many` | An 8-by-8 array of disjoint 4 mm squares with 8 mm spacing: 64 boundaries and 64 material regions per plane; 512 source events per plane. |
| `face-heavy` | One 4 mm square ribbon plus 50,000 deliberately duplicated triangles entirely at Z=2: exactly 50,008 scanned faces, 50,000 non-intersecting faces and eight retained events per plane; one region. Duplicate source faces are preserved. This is a scan workload, not a valid-solid or printability claim. |
| `construct-overlap` | Twenty-four 8-by-8 integer rectangles translated by `(i,i)`, each its own explicit material: connected, orthogonally convex union, one outer boundary and no holes. |
| `construct-rational` | Two overlapping convex triangles `[(0,0),(6,0),(0,6)]` and `[(1,-1),(5,7),(7,-1)]`, distinct materials: one hole-free region, with a constructed noninteger coordinate. |
| `pair-cap` | The same six triangle edges require exactly 15 unordered pairs: budget 14 must reject with the typed pair limit; independent validation outside timing checks budget 15 succeeds. |

The worker checks contiguous plane indices and exact expected Z bits, complete strict
layers, independent region/boundary/event/face counts, and unchanged schedule length.
Constructive results additionally check boundary-atom accounting and their side dispositions.
The rational-coordinate check uses bounded `u128` fixture values; it is not production
coordinate conversion. The broader correctness corpus remains in automated tests.

The first observation of these workloads (Linux/WSL Release, 2026-10-17) is in
[docs/TOOL-RECORDS.md](../../docs/TOOL-RECORDS.md#m3-measurement-protocol-initial-linuxwsl-release-observation).

## TD-031 represented-coordinate cases

`represented-nested` and `represented-many` reuse the corresponding analytic integer-coordinate
ribbon meshes but opt into represented millimeter topology over **seven** planes from -0.75 through
0.75 mm at 0.25 mm intervals. These include the formerly rejected quarter planes; every batch
checks completeness, analytic boundary/region counts, plane order, mode and source accounting.
The original three-plane exact-grid cases remain available unchanged.

```console
target/release/examples/m3_profile timing represented-nested 4
target/release/examples/m3_profile timing represented-many 1
/usr/bin/time -v target/release/examples/m3_profile memory represented-many 1
```

Their results are in [docs/TOOL-RECORDS.md](../../docs/TOOL-RECORDS.md#m3-measurement-protocol-td-031-represented-coordinate-results).

## M3 closure large-input cases

The closure protocol in `docs/M3-CLOSURE-ACCEPTANCE.md` fixes budgets before execution:
5 seconds maximum per complete operation, 64 MiB retained output, and 512 MiB whole-process
peak RSS in each of three memory runs on the recorded Linux Release host. New cases are:

- `reference-large`: the independently generated 327,680-triangle FX-11 sphere, three represented
  layers at Z=15/30/45 mm, one region each and 0.1% ideal-sphere area envelope.
- `face-million`: one square ribbon and 1,000,000 nonintersecting duplicate faces, three layers;
  full face accounting must remain exact. This is scan stress, not a valid solid claim.
- `regions-large`: 1,024 disconnected square ribbons, three layers and 1,024 regions per layer.
- `construct-large`: 64 independently labelled overlapping rectangles; one outer and no holes.

The worker uses an explicit two-million face cap and 40-million validity-pair cap for the large sections, plus a 100-million constructor work cap for the 64-source union, leaving
library defaults unchanged. Build with the existing Release command. Generate FX-11 if it is absent using
`cargo run --release -p reference-fixtures -- --output-dir /tmp/resin-m3-fixtures`, then copy only
`stl/fx-11-high-complexity.stl` into the ignored local fixture path; never overwrite tracked fixtures.
The automated acceptance test independently generates FX-11 in memory and needs no local file.
Run `timing CASE 1`, then `memory CASE 1` in three separate GNU-time processes as above. Setup/import
is outside timing but inside process memory. All samples must pass analytic checks; report the
maximum timing and all three RSS values rather than selecting a favorable sample. The final closure
record contains the executed results and limitations.
