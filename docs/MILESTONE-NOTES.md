# Engine milestone notes

Moved from the repository README on 2026-11-10 (owner): the README keeps what a reader needs to
build and use the software, and points here for the rest. ROADMAP.md holds the milestones' status;
these are the summaries of what each engine milestone added, with links to their specifications
and validation records.

## Status summaries (M2 to M5.5)

karasu-slicer is an independent, cross-platform MSLA/LCD resin slicing application written in
native Rust. The project prioritizes geometric correctness, numerical robustness, deterministic
behavior, memory safety, maintainability, and then measured performance.

The repository has completed M2: M2.1 mesh/STL import, M2.2 native scene transformations, M2.3 source
topology: bounds, explicit pivots/rotation/scale/composition, directed edge incidence, boundary and
non-manifold diagnostics, local winding, two component definitions, vertex-link analysis, cooperative
cancellation, read-only CLI inspection, and deterministic transform round trips. M1 research remains
open independently. M3.1 implements the local triangle–horizontal-plane intersection kernel under
its committed mathematical contract. M3.2-A adds a bounded, cancellable full-mesh raw section at one
explicit world-space plane, preserving indexed provenance and face multiplicity. M3.2-B adds
non-destructive source-feature normalization and conservative local coplanar ownership, retaining
conflicts and ambiguous incidences. M3.3-A/B add an incidence graph and conservative ordered paths.
M3.4 adds planar inspection, area/orientation and exact-grid mapping; M3.5 adds strict region
hierarchy and explicit uniform planes; M3.6 adds owned layer generation, progress and atomic
collection. M3.5-C adds explicit source-material exact composition. M4 adds bounded LCD rasterization, configurable midpoint AA, grayscale compensation, preview metadata
and deterministic reference PNG export. M5 adds validated print jobs, profiles and versioned machine
exporters. M5.5-A adds preparation plan/history contracts and M5.5-B adds bounded convex
hollowing. M5.5-C adds bounded finite drainage cuts and M5.5-D adds parametric manual support
geometry. M5.5-E completes bounded combined integration, revision-safe worker publication and
progress; a graphical interface remains M6 work. The executable provides read-only STL inspection,
headless slicing of one STL into a machine file or reference PNG directory (TD-043),
identification, version, and help output.


M3 is accepted for the [documented corpus and budgets](M3-CLOSURE-ACCEPTANCE.md), including
represented-coordinate regions and coherent on-plane shared-edge crossings. Stable Linux passes
611 tests and the complete native Windows MSVC Release suite passes 610; both retain two ignored
tests, with the difference being Unix-only CLI coverage. At the M3 closure revision, Windows Debug launch was restricted by host Code Integrity;
M4 subsequently passes complete native Debug and Release suites as recorded below. M1
commercial-default research remains open. M4 is complete for the documented engine contract: Linux **645/0/2**, native Windows Debug
and final complete Release **644/0/2**. See the [LCD contract](M4-LCD-IMPLEMENTATION.md);
see its [validation record](M4-VALIDATION.md) for platform acceptance and research limitations.

## Toolchain and native Windows notes (M0, M2)

The WSL environment inspected during M0 did not have Rust or rustup installed. Install or make a
stable Rust toolchain available before running the commands below. Toolchain installation is an
environment operation and is not performed by this repository.

Final native Windows validation of accepted M2 baseline
`d8816303bbc4c2ba26213645ddf278db6574342b` passes formatting, strict Clippy, workspace tests/build,
warnings-denied rustdoc, Closures A–C, representative CLI inspection, and fixture/source-integrity
checks. Windows reports 223 passed, zero failed and two ignored versus Linux/WSL 224/0/2; the single
difference is the intentionally Unix-only non-Unicode-path regression. These recorded results do not
make later unvalidated changes Windows-compatible automatically.

## LCD masks and reference PNGs (M4)

`karasu_core::raster::rasterize_layer` consumes a complete M3 `RegionLayer` or its explicitly attached
constructed material union. `LcdProfile` requires physical origin/extents, pixel dimensions and
orientation. `RasterConfig` explicitly selects midpoint AA (1/2/4/8/16 per axis), 2..256 grayscale
levels, clipping/rejection and optional rectangular pixel dilation/erosion. No commercial printer,
gamma, sampling or compensation defaults are inferred. Unresolved geometry fails explicitly.
`raster::stream::RasterStream` produces one owned mask at a time without retaining a layer cache;
images expose layer/physical metadata and intensity statistics for headless previews.

The dependency-free `raster::png::write_png` writes deterministic grayscale8 reference images with
bounded stored-DEFLATE scratch. Export via the analytic reference example:

```console
cargo run --release -p karasu-core --example raster_reference -- 512 320 4 /tmp/karasu-reference.png
python3 tools/m4-profile/validate_png.py /tmp/karasu-reference.png --samples 4
```

The validator independently decodes PNG framing/checksums with Python zlib and compares every pixel
with an analytic point-sampling oracle. The example is a benchmark/reference tool; the main CLI
continues to expose inspection/help/version. See [M4 specifications](M4-LCD-IMPLEMENTATION.md),
[PNG design](M4-PNG-DESIGN.md) and [benchmark instructions](../tools/m4-profile/README.md).

## Print jobs, profiles and export (M5)

The headless core now provides immutable validated jobs, printer-bound resin calibration, versioned
user profile storage and streaming exporters. Registered dialects are CTB v2 plain, GOO v3,
firmware-motion Photon v1 and single-material Photon P1 PP1 v3. Format IDs and explicit grayscale
loss policy select capabilities; extension alone does not establish printer compatibility.

```console
cargo run --release -p karasu-core --example print_reference -- /tmp/karasu-print-sample
python3 tools/m5-validate/validate_pp1.py /tmp/karasu-print-sample/sample.pp1 --reference-dir /tmp/karasu-print-sample/reference
```

The example uses uncalibrated fixture settings. It is not a printer preset or a physical print
recommendation. Main CLI commands remain inspection/help/version. See [M5 specification](M5-IMPLEMENTATION-SPEC.md),
[PP1](M5-PP1-SPEC.md) and [validation](M5-VALIDATION.md) for supported subsets and evidence.

## Preparation plan contracts (M5.5-A)

The headless `preparation` module provides validated immutable operation plans, exact-content support
bindings, canonical replay identity and bounded transactional undo/redo. The separate M5.5-B backend
below adds convex hollowing, C adds finite drainage scenes and D adds parametric support geometry.
Combined headless preparation is accepted in [M5.5 closure](M55-CLOSURE-ACCEPTANCE.md). See
[specification](M55-IMPLEMENTATION-SPEC.md) and [validation](M55-VALIDATION.md).

## Convex model hollowing (M5.5-B)

`preparation::hollow::hollow_mesh` constructs a real inward cavity and sliceable shell for one closed,
outward convex polyhedral source. It preserves the placed exterior and certifies an explicit erosion
error bracket; unsupported concave/multiple/invalid sources fail rather than being silently repaired.
No foreign geometry kernel or new dependency is used. See [contract](M55B-HOLLOWING-SPEC.md),
research and [validation](M55B-VALIDATION.md).

```console
cargo run --release -p karasu-core --example hollow_reference -- /tmp/karasu-hollow-cube
python3 tools/m55b-validate/validate_hollow.py /tmp/karasu-hollow-cube --cube
```

The example writes owned analytic fixtures and f64 reference geometry, not calibrated print files.
Parametric support geometry is described below; combined headless acceptance is recorded in [closure](M55-CLOSURE-ACCEPTANCE.md).

## Finite drainage cuts (M5.5-C)

`preparation::drainage::drain_model` certifies finite axis-parallel openings into the convex cavity
and evaluates real drained material by exact layer-domain subtraction. It returns a prepared scene,
not a final Boolean triangle mesh. Entry and terminal caps require certified clearance; oblique,
shallow, missed, potentially overlapping and uncertain cuts fail explicitly. Geometric access does
not guarantee physical resin removal. See [contract](M55C-DRAINAGE-SPEC.md) and
research and [validation](M55C-VALIDATION.md).

```console
cargo run --release -p karasu-core --example drain_reference -- /tmp/karasu-drain-cube
python3 tools/m55c-validate/validate_masks.py /tmp/karasu-drain-cube
```

The example writes synthetic reference masks, not calibrated printer exposure settings.

## Parametric manual supports (M5.5-D)

`preparation::supports` generates support geometry from independent polygonal sections with world
positions, sizes and full tilt/roll. Elliptical polygons, rectangles, regular/custom convex polygons
and sharp endpoints control base, shaft, neck, taper, bends and tip. Immutable recipes/history retain
exact model revisions. Geometric certificates verify intended contact, platform/path connection and
conservative collision clearance. Actual layer material unions supports with the hollowed/drained
model; sources remain in the documented convex domain. This is headless geometry, without a strength
guarantee or final Boolean mesh export.

```console
cargo run --release -p karasu-core --example support_reference -- /tmp/karasu-support-fixture
python3 tools/m55d-validate/validate_support.py /tmp/karasu-support-fixture
```

See [specification](M55D-SUPPORTS-SPEC.md), research
and [validation](M55D-VALIDATION.md). The example exports synthetic uncalibrated geometry/masks
and CTB/GOO/Photon/PP1 fixtures; it supplies no printer exposure recommendation.

## Revision-safe preparation integration (M5.5-E)

`preparation::session::PreparationSession` binds immutable source and bounded history to owned
worker snapshots. Only a newest matching outcome can become current; stale/foreign results and
edit→undo ABA cases are rejected. Failed/cancelled work preserves a labeled preview of the last
completed plan while blocking current scene access. Progress reports named stages/counts; computed
Complete does not mean session acceptance. No scheduler or GUI dependency is added.

The same imported owned STL is hollowed, drained, supported, sliced and exported through all four
machine formats. Independent readers validate geometry and 1.6 million combined pixels, plus an
eight-support/24-component workload. Full Linux Debug/Release passes **852/0/2**; native Windows
MSVC passes **851/0/2**. See [specification](M55E-INTEGRATION-SPEC.md),
[closure and limitations](M55-CLOSURE-ACCEPTANCE.md) and
[reproduction tools](../tools/m55e-validate/README.md). M6 desktop integration is next; M7 remains required.
