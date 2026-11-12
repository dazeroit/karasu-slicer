# M5.5-D parametric support validation

## Result and scope

On 2026-10-18 the headless parametric manual support backend passes the documented software gates.
It generates explicit closed convex-loft components, certifies platform/path/intended model contact
and conservative collision clearance, and evaluates actual support/model material through M3/M4/M5.
Sources are one certified convex solid or the existing bounded hollow/drain domain. Arbitrary concave
sources, automatic placement/trees, analytic smooth sweeps, final Boolean boundary mesh export and
mechanical/physical printing guarantees are excluded. See [contract](M55D-SUPPORTS-SPEC.md).

Research records the controls other programs offer, which are not runtime algorithm guarantees.
No other program's implementation, foreign geometry kernel, new dependency or reference asset is
imported. Raw evidence remains outside the repository.

The implementation is based on `181e5f94d70c263a79b1934bbf483929cf63a44f` with reviewed working
changes. Historical SupportSpec/KPREP1 behavior is retained. New complete parametric KSUP1 identity,
semantic tolerance matching and cancellable bounded history are covered independently.

## Complete quality gates and source integrity

Final Linux passes all required commands plus complete Release and warning-denied documentation:

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo test --workspace --release
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
```

Each complete profile reports **828 passed, zero failed, two ignored**. Results/logs are in
`/tmp/karasu-m55d-validation/results.json` and the corresponding command logs. Existing union,
difference, hollowing, drainage, import, raster, profile and export regressions remain enabled.
Six independent Python reader unit tests pass separately.

Native Windows uses the documented local-MSVC hash-verified snapshot runner, including reviewed
untracked files: 262 files, archive SHA-256
`68c0cd1d9d6c98301b1b71f502b362b194ada1422a23187c58cda612eccbaf2d`.
Native fmt, strict all-target Clippy, complete Debug/Release suites, build, public/private rustdoc,
Release support example build and complete reference/machine export all pass: **zero failed commands**.
Each native profile reports **827 passed, zero failed, two ignored**; the Unix-only path test explains
the count difference. Before/after source integrity checks pass. No application-control launch block
occurred and no persistent security policy was modified.

Input/runner provenance: `/tmp/karasu-m55d-native`. The temporary generic runner copy adds only the
support example build/run to its existing sequence. Native evidence directory:
`%LOCALAPPDATA%\Temp\karasu-native-7182e070cdcf4ecc98bff4a7761c76a5\evidence-b059140a6c8d46dc8ec4c366594996dc`.
Executable Rust, tests, examples, Cargo and lock inputs remain identical to the validated snapshot.
Later edits complete documentation/status/evidence narratives only; they are not claimed to have
been compiled in that snapshot. Raw local temporary evidence is not promised permanent storage.

## Geometry, recipe and exact material coverage

New stable coverage comprises:

- Eight immutable plan/history tests: full upstream identity, shapes/centers/rotation/semantic
  tolerance changes, stale anchors, count/bytes/work failure, horizontal/returning paths, add/edit/
  removal, undo/redo and transactional cancellation preserving generation/current/redo.
- Fifteen public geometric support tests: real analytic base/shaft/tip material sections, source and
  plan immutability, solids without invented cavities, hollow and drained models, custom/elliptical/
  regular/rectangular profiles, rotated/bent sections, sharp endpoints, explicit support union,
  missed/deep/cavity/model/cutter contact rejection, stale revisions, resources and cancellation.
- A genuine horizontal side-contact test: a fully oriented YZ tip traverses source X=0 at constant
  center Z=18, has certified penetration, retains closed component edges and contributes real layer
  material outside the model while leaving the cavity empty.
- Five private profile tests: exact convexity/origin admission, full section placement, side-count
  limits and post-placement exact rank checks. Tiny point- and line-collapsed profiles are rejected;
  representable tiny profiles are not rejected by an unexplained global epsilon.
- Seven prepared-arrangement tests: independently computed areas/cell occupancy for body parity,
  holes, absent body, additive overlap, cutter/additive coincidence, contact, winding/source order,
  dyadic coordinates, cancellation/budgets and both layer coordinate modes. A cutter cannot silently
  remove additive support material.

Within-support shared-section connectivity, one-facet tip penetration and unintended obstacle
separation are supported by exact/interval predicates, not sampled rays or bounding-box collision
claims. A missing separating certificate is an explicit uncertainty failure. Explicit overlap union
is not reported as a strength certificate. Default aggregate admission is conservative, including
subordinate maxima and copied buffers; independent per-field maxima need not fit simultaneously.

## Independent artifact and M5 integration validation

The owned fixture is an original 20 mm cube translated upward 10 mm, hollowed by 2 mm, with one
rectangular support centered at world XY=(12,8). Four stations at Z=0,1,9,10.3 use full section sizes
4,4,1,0.4 mm. Three independent components have volumes about 16, 56 and 0.676 cubic millimeters;
exact rational represented-volume checks are authoritative over rounded decimal descriptions.
The nominal contact is on the bottom facet with maximum penetration 0.5 mm. The source stamp is a
synthetic caller assertion, not a verified asset-byte digest.

`tools/m55d-validate` independently checks actual component coordinates, oriented closed edges,
connected boundaries, positive exact represented volume, exhaustive vertex/face convexity and
owned station corners. It does not replace the full Rust embedding/contact certificate, establish
all vertex links/coplanar contacts or validate arbitrary support topology.

Five 200x200 binary PGM layers at Z=0.5,5,9.5,11,13 contribute **200,000 exact analytic pixel checks**.
A uniform five-layer M3→M4→M5 stream at Z=0.5,5,9.5,14,18.5 contributes another **200,000 reference
PNG sample checks**. All 400,000 pass with no mismatched or indeterminate-band allowance. Independent
M5 readers decode CTB v2 plain, GOO v3, Photon v1 and Anycubic PP1 v3 and compare all masks with the
validated reference PNGs. Geometry, pixels and machine files are synthetic software fixtures; the
200x200 panel and 4.5 mm export layer spacing are explicitly uncalibrated and not printer presets.

All three final Linux artifact sets and the native Windows set pass the full reader. Repeated Linux
geometry/masks/reference/machine bytes match exactly; all report fields repeat except
`geometry_elapsed_ms`. Native geometry JSON, all PGM/reference files, CTB, GOO and Photon bytes match
Linux exactly. Every PP1 ZIP member also matches except the documented OS value in
`software_info.conf` (`linux` versus `windows`); all its other metadata match. Reports match except
the timer. Results are retained in `/tmp/karasu-m55d-validation/native-comparison.json` and
`native-independent.json`.

An initial development export rejected a synthetic machine name beyond GOO's 31-byte ASCII limit;
its partial artifacts remain in `/tmp/karasu-m55d-final-fixture`. The shorter explicit fixture name
passes without changing or weakening any exporter validation. Final profiling uses fresh directories
and the fully frozen source.

## Preliminary fixture budgets

[Budgets](../tools/m55d-validate/PROFILING.md) were declared before timed measurement: at most 10 seconds
for geometry generation/admission/certification and 512 MiB whole-process peak RSS for the full owned
example. Three fresh Release processes pass:

| Run | Geometry evaluation (ms) | Whole-process RSS (KiB) | Full independent reader |
| --- | ---: | ---: | --- |
| 1 | 25.891 | 4096 | Pass |
| 2 | 22.141 | 4224 | Pass |
| 3 | 22.070 | 4096 | Pass |

The geometry timer excludes source/plan creation, layer evaluation, rasterization, file serialization
and exporters. GNU time RSS covers the entire process including those steps. Compiler/test activity
ran concurrently; scheduling/caches/frequency are uncontrolled. These are preliminary owned-fixture
budgets, not isolated throughput, large-scene guarantees, repeated native profiling or commercial
slicer speed comparisons. Source/executable/environment hashes and complete reports are in
`/tmp/karasu-m55d-bench`.

## Bounded fuzz and remaining milestone

Final-source Linux ASan, seed 5505, passes **103 executions in 21 reported seconds**, peak RSS 68 MiB,
exit zero and no failure artifacts. All core/test/example/harness/Cargo hashes match preflight and
postflight. Fixed replay passes 33 inputs (13 accepted/20 expected rejections); the retained 40-input
corpus passes (20 accepted/20 rejected). Typed rejection is expected. Fuzz formatting, strict all-target
Clippy and fixed-seed unit checks pass. Protocol, hashes, corpora and logs are external in
`/tmp/karasu-m55d-fuzz`; reproduction/limits are in [fuzz documentation](FUZZ-CAMPAIGNS.md).
The campaign requests 20 seconds, 256-byte inputs, 1024 MiB RSS and 10-second per-input limits. Leak
detection is disabled for the established host ptrace constraint; address instrumentation is active,
but no leak-clean or native Windows sanitizer claim is made. This short bounded campaign is not
exhaustive profile/contact/numeric coverage or sustained adversarial acceptance.

M5.5-D closes only its declared backend domain. Comprehensive hollowed/drained/supported scene
acceptance, composed error/performance/stress coverage and M6 handoff remain M5.5-E. Interactive
placement/editing, asynchronous revision orchestration, packaging and hardware/physical acceptance
remain M6/M7. M5.5 as a whole is not yet closed.
