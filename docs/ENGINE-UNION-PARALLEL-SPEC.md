# Winding-union layers and layer parallelism

## Status and purpose

**Status (2026-10-18):** approved by the owner with three decisions:
- the CLI default fill becomes `union` once the mode passes U1–U6;
- the CLI uses all available cores;
- layer parallelism is implemented first.

Both parts are **implemented** (see "Implementation status"): winding-union layers, now the CLI
default, the parallel layer stream and parallel per-layer encoding. TD-050 records the decisions.

The engine-versus-commercial analysis of 2026-10-18 found two gaps that matter most for
competitiveness. Both are outside correctness, where the engine already leads:
1. **Input tolerance.** The strict pipeline refuses layers where bodies touch, overlap or
   self-intersect, and its even-odd rule would turn an overlap into a hole. Real models (sculpted
   miniatures, multi-body exports, kitbashes, models with merged supports) routinely contain such
   geometry. CHITUBOX and Lychee slice them.
2. **Throughput on 12K/16K panels.** A 20 mm cube at 0.05 mm (400 layers) on a 13312 × 5120 panel
   took 164 s on one core (0.41 s per layer, 70 MB peak). A typical 1200-layer job would take
   about 8 minutes, while the host has 16 cores.

This document specifies two independent changes:
- an explicit **winding-union layer mode** that slices such inputs without repair and without
  silent reinterpretation;
- **parallel layer generation** that is byte-identical to the sequential stream.

## Part 1 — Winding-union layers

### Semantics

For a layer plane `z`, the material at a point `p` of the plane is defined by the winding number
`w(p)` of the plane section of the oriented source surface:
- each face crossing contributes a directed segment with material on its left (outward faces);
- `w(p)` is the signed number of segments a ray from `p` crosses.

The filled set depends on an explicit fill rule:

| Rule | Filled when | Use |
| --- | --- | --- |
| `Positive` (default of the mode) | `w > 0` | Union of outward bodies; reversed (inward) shells subtract. A hollow shell (exterior plus reversed cavity) gives `1 - 1 = 0` inside the cavity. |
| `NonZero` | `w != 0` | Compatibility with sources whose orientation is globally reversed. |
| `EvenOdd` | `w` odd | The existing strict semantics, for comparison. |

On a closed embedded surface (one shell per body, no contacts) the three rules agree with the
strict pipeline. **The mode must produce byte-identical masks to the strict pipeline on every
input the strict pipeline accepts** (acceptance criterion U1).

### Why this is not repair

- No vertex, face or contour is moved, merged, added or dropped.
- The winding number is a function of the represented surface and the declared rule, the same
  quantity CHITUBOX and Lychee implicitly use.
- Every layer reports the facts that distinguish it from a strict layer: whether contours cross,
  touch or coincide, and the extreme winding values. Callers can show these as diagnostics.
- Anything the rule cannot define fails explicitly (see Admission).

### Per-layer construction

1. **Plane.** Union mode always uses vertex-avoiding planes (TD-048). No vertex lies on the plane,
   so every face is either missed or crossed strictly through exactly two of its edges. Coplanar
   faces, on-plane edges and on-plane vertices cannot occur.
2. **Segments.**
   - Each crossed face yields one segment between its two edge-crossing points.
   - A crossing point is computed from the canonical (sorted-vertex) edge, so the two faces sharing
     an edge obtain bit-identical endpoints and the section has no cracks.
   - The segment's direction comes combinatorially from the face's cyclic vertex order and which
     vertex lies alone on one side of the plane. No arithmetic sign is involved, so the orientation
     is exact.
3. **Balance check** (admission, exact).
   - Endpoints are keyed by canonical edge identity, not by coordinates. At every key, outgoing and
     incoming segments must balance.
   - Closed surfaces, including non-manifold edges, self-intersecting shells and overlapping bodies,
     always balance.
   - An unbalanced key exists only at an open boundary edge or at an edge with inconsistently
     oriented faces.
4. **Rasterization.**
   - Signed crossings replace parity crossings: per sample row, crossings are sorted by X and the
     running winding defines filled spans under the rule.
   - Supersampling, grayscale quantization, compensation, cut rings (drainage) and outside policy
     are unchanged.
   - Cost is linear in segments plus pixels, as today.
5. **Report.** Per layer the mode reports:
   - segment count;
   - minimum and maximum winding over sampled spans;
   - counts of crossing segment pairs (from the scanline sort, no extra pass);
   - a flag when the rule merged overlapping material (`max w > 1` under `Positive`).

The normalization, incidence graph, path, planar-validity, area and region stages are not run. Their
strict outputs remain available through the existing mode.

### Admission and failures

- **Unbalanced layer.** The layer fails with the offending canonical edges (bounded detail list)
  and the reason: open boundary, or inconsistent orientation. It is never filled by a guessed
  closure.
- **Open meshes.** They remain refused in this mode unless a separate future policy is approved,
  for example generalized winding with an explicit threshold, which is O(faces) per sample and
  would need its own budget. `--allow-topology-defects` does not change the union rule.
- **Inconsistent orientation.** It is reported, not flipped. A globally reversed source is sliced
  correctly under `NonZero`, or after an explicit orientation fix by the user.
- **Limits.** These are the existing section limits, raster limits and cancellation checkpoints;
  segment and key buffers are reserved fallibly against the layer payload cap.

### Interaction with preparation

- **Hollow shells.** They are already exterior plus reversed cavity: `Positive` gives the wall.
- **Drainage.** Cut rings are subtracted per sample after the winding fill, as today.
- **Supports (future).** They become additive shells under `Positive` without exact construction
  (the M3 constructive union keeps its smaller exact domain).

### Interfaces (proposed)

- `LayerEngine::with_winding_fill(rule: WindingRule)` produces `WindingLayer` items: the plane,
  the sampled Z, the oriented segments and the report. It implies vertex-avoiding planes.
- `raster::rasterize_winding_layer[_minus_rings]_with_cancellation` and a
  `WindingRasterStream` / `WindingDifferenceRasterStream` mirror the existing streams.
- CLI: `--fill strict|union` (default to be decided by the owner, see Open decisions). The
  summary prints the number of layers where overlaps were merged.

### Validation

| ID | Check |
| --- | --- |
| U1 | Byte-identical masks to the strict pipeline on the full M3/M4 corpus and FX-11 (solid, hollow, drained). |
| U2 | Two overlapping cubes, touching cubes (shared face), a kitbash of interpenetrating primitives, a self-intersecting single shell: masks equal a brute-force per-sample winding-number oracle on small panels. |
| U3 | A hollow shell, a preserved inner void and nested reversed shells: voids stay empty under `Positive`. |
| U4 | An open mesh and a flipped face are refused with the offending edges; no mask is produced. |
| U5 | Plane positions on vertex heights (TD-048 cases) and tangent configurations never fail. |
| U6 | Randomized fuzz: random overlapping closed bodies, rule `Positive` against the oracle. |

## Part 2 — Parallel layer generation

### Goal

Use all cores for section, fill and rasterization, with output byte-identical to the sequential
stream for every thread count. Indicative target: a 1200-layer 12K job in under 60 s on 16 cores,
down from about 8 minutes.

### Design

1. **Independent layer functions.** Layer `k` is a pure function of the immutable mesh, transform,
   schedule plane `k`, configuration and (for drainage) the cutter set. Workers compute
   `section → (strict regions | winding segments) → raster image` for whole layers.
2. **Immutable Z index.** The current `FaceZSweep` is a mutable sweep that requires increasing
   planes. Replace it for parallel use by an immutable stabbing structure built once:
   - faces sorted by minimum Z;
   - a centered interval tree (or a static segment tree over the sorted maxima).
   A plane query returns the candidate faces in ascending source order in `O(log F + k)`.
   Candidates, and therefore events, stay identical to the full scan.
3. **Scheduling.** Workers claim contiguous chunks of layers from an atomic counter (chunk size
   about 4, so sweeps stay local and claims stay cheap). They run on `std::thread::scope`, with no
   new dependency.
4. **Reorder buffer.** Results enter a bounded window keyed by layer index. The consumer (exporter,
   progress, CLI) receives them strictly in index order through the existing iterator interfaces,
   so encoders and file layout are unchanged.
5. **Memory bound.** At most `W` layers are in flight, with
   `W = min(2 × threads, floor(budget / per_layer_bytes))`. Here `per_layer_bytes` is the raster
   preflight (dominated by the image: 68 MB on a 13312 × 5120 panel) and `budget` is a new explicit
   limit (default 1 GiB). Workers block when the window is full. `W >= 1` always; with one thread
   the stream is the sequential one.
6. **Parallel encoding (second step).** Machine formats encode each layer independently except for
   offsets and checksums. An `encode_layer` that returns owned bytes can run in the workers, and
   the writer then only appends bytes and records offsets in order. This removes the remaining
   serial run-length encoding cost.

### Cancellation, errors and progress

- **Cancellation.** The user predicate is called only on the consumer thread (it is not `Sync`).
  The consumer forwards a stop request to an `AtomicBool` that workers poll at their existing
  checkpoints; cancellation latency is that of the slowest in-flight checkpoint.
- **Errors.**
  - The first error in **index order** wins, so error reporting is deterministic.
  - When layer `k` fails, layers `< k` are still delivered, workers stop claiming chunks, and later
    results are discarded.
  - A worker panic is caught at the scope boundary and reported as an internal error; it never
    yields a partial file.
- **Progress.** It is reported by the consumer in index order, exactly as today.

### Determinism

Every per-layer computation is already deterministic and independent of thread count (hollowing
uses the same rule). The parallel stream adds no reduction across layers, so outputs are
byte-identical by construction. The tests check it explicitly.

### Validation

| ID | Check |
| --- | --- |
| P1 | Byte-identical machine and reference outputs for 1, 2, 4, 8 and 16 threads on the corpus, FX-11 (solid, hollow, drained) and the union-mode fixtures. |
| P2 | Deterministic first error in index order with induced failures at chosen layers; no partial publication. |
| P3 | Cancellation at every checkpoint with several threads publishes nothing, with bounded latency. |
| P4 | Peak memory stays within the window budget at 13312 × 5120 (measured RSS). |
| P5 | Throughput benchmarks: FX-11 at 1000 × 600 and a 1200-layer job at 13312 × 5120, single- and multi-threaded, recorded in PERFORMANCE-BASELINE. |

## Implementation status

**Winding-union layers (implemented).** `slicing::winding` and `raster::parallel::LayerFill`:
- **Construction:** `LayerKernel::winding_layer` sections the vertex-avoiding plane with the
  existing section kernel (and the shared face Z index) and orients each crossed face's segment
  combinatorially from its lone corner. A reflecting source transform reverses every segment; a
  singular one fails (`LayerFailure::SingularTransform`). None of the strict stages after the
  section run, so union layers are also faster to build.
- **Balance check:** keyed by the exact crossing position rather than by edge identity. Faces
  sharing an edge produce the bit-identical point, so for welded meshes this is the edge check of
  the proposal; it also accepts unwelded duplicate vertices, and positional balance is exactly the
  condition for the winding number to be defined. Failures (`LayerFailure::Unbalanced`) give the
  total count and up to 16 crossings in XY order, each with one crossed mesh edge and its excess
  (odd: open boundary; even: inconsistent orientation).
- **Raster:** `raster::rasterize_winding_layer_with_cancellation` (with optional cut rings) sweeps
  signed crossings per sample row. A mirrored panel orientation (`swap_xy`, `flip_x`, `flip_y`
  with odd parity) negates the winding. Images report the sampled winding range
  (`RasterImage::winding`); under `Positive` a negative sampled winding number fails the layer
  (`RasterError::NegativeWinding`: an inside-out model or an unenclosed reversed shell), so an
  inside-out model never prints as empty layers.
- **Exact sampling:** for both fills, each crossing's first sample column is fixed by the exact
  orientation sign of the sample centre against the pixel-space edge, starting from an
  interpolated estimate. Membership is then independent of edge direction and rounding, which
  makes the union masks equal the strict masks by construction (U1), not by rounding luck. The
  existing strict outputs (corpus, FX-11, 16K) are unchanged and the cost is not measurable.
- **Not implemented:** counts of crossing segment pairs in the per-layer report (only the sampled
  winding range is reported), and `LayerEngine::with_winding_fill` plus dedicated sequential
  streams: winding layers run through `with_parallel_raster` (one thread gives the sequential
  order), which the CLI and tests use.
- **CLI:** `--fill union|strict`, default `union` (positive rule). Union admission refuses meshes
  with boundary edges or winding conflicts unless `--allow-topology-defects` is given, and accepts
  non-manifold edges and vertices (touching bodies). The summary prints the fill and the number of
  layers whose fill merged overlapping material.

Validation (`tests/winding_layers.rs`, CLI tests):

| ID | Result |
| --- | --- |
| U1 | Masks identical to strict for every rule on the closed corpus FX-01–07 and FX-11 at 1 and 16 samples per pixel; CLI files identical for FX-11 solid, hollow and drained, a box and a step model. |
| U2 | Overlapping boxes, boxes sharing a face, a kitbash (box, sphere, rotated box) and a self-intersecting pentagram prism equal the generalized-winding-number oracle (solid angles) under every rule; strict refuses them. |
| U3 | A cavity with a body floating inside it and a hollow sphere match the oracle; the void stays empty. |
| U4 | An open box and a flipped face fail with the offending edges; no mask is delivered. |
| U5 | Planes exactly on the step heights of a staircase never fail and equal strict. |
| U6 | Twelve random sets of rotated boxes and a sphere match the oracle under every rule. |

Also: inside-out sources fail under `Positive` and fill under `NonZero`; mirrored panels and 1, 2
and 7 threads give the strict masks.

| Case (union, CLI) | Strict | Union |
| --- | --- | --- |
| FX-11 solid, 1000 × 600 | 0.85 s | 0.72 s |
| FX-11, 2 mm hollow | 2.97 s | 2.58 s |
| FX-11, 2 mm hollow and 2 drain holes | 3.82 s | 3.38 s |

**Parallel layer stream (implemented).** `raster::parallel::with_parallel_raster` and
`ParallelRaster`:
- **Workers:** each runs the sequential engine's own per-layer function (`LayerKernel`), so the two
  paths cannot diverge, then rasterizes the layer (minus analytic cuts, if any).
- **Index:** the face Z index is split into an immutable `FaceZIndex`, shared by all workers, and
  one `FaceZCursor` per worker. Each worker claims increasing layer indices, so its cursor only
  sweeps forward.
- **Window:** results enter a reorder window of `min(2 × threads, budget / per-layer preflight
  bytes)` layers (default budget 1 GiB) and are delivered in layer order.
- **Errors and cancellation:**
  - the first error in layer order wins, and later layers are abandoned at their next checkpoint;
  - the caller's predicate is polled by every worker;
  - an early consumer stop joins all workers;
  - a worker panic becomes `LayerFailure::WorkerPanicked`.
- **CLI:** uses it with all cores for solid, hollow and drained models.

Measured in Release on the 16-core host. All outputs are byte-identical to the sequential pipeline.

| Case | Sequential | Parallel |
| --- | --- | --- |
| 20 mm cube, 400 layers at 13312 × 5120 (GOO) | 164 s, 70 MB | 36 s, 1.07 GB (window) |
| FX-11, 2 mm hollow and 2 drain holes, 1000 × 600 | 12.5 s | 4.5 s |
| FX-11, 2 mm hollow, 1000 × 600 | 11.7 s | 3.7 s |

At 13312 × 5120 only about 4.7 cores stayed busy at first: machine encoding (run-length coding of
68 Mpx per layer) ran on the consumer thread.

**Parallel encoding (implemented).** The machine codecs separate three steps:
- **precheck:** session state (layer count, Z order and build height, per-layer capability);
- **encode:** pure (format quantization, intensity sum, run-length or PW0 payload);
- **emit:** volume, records and bytes, bookkeeping.

`write_layer` composes them in the original order. `ExportSession` gains `layer_encoder` (a
thread-safe `LayerEncoder`) and `write_encoded`. Both have defaults, so other exporters still work
and write sequentially.

`export::export_machine_parallel`:
- **Ordering:** pulls, checks and accumulates layers in order on the calling thread, exactly as
  `export_machine`; workers encode within a window bounded by `ParallelEncoding::window_bytes`
  (default 512 MiB of source masks); results are written strictly in order.
- **Errors:** a pull failure is held until every earlier layer is written, so the published bytes,
  the summary and the first failure are identical to `export_machine`.
- **Formats:** PP1 still records its scene, projection and ZIP entries in order on the calling
  thread; only PW0 encoding runs in parallel.

**Span rasterization (implemented).** Each row is built from the sorted sample-space endpoints of
its sub-row spans (one opening and one closing event per span). Between consecutive endpoints the
number of covering sub-rows is constant, so whole pixels are written as constant runs and only the
partially covered pixels at run ends accumulate. Uncovered pixels are never visited (the image
starts at zero), and without compensation the preview statistics are gathered from the written runs
instead of a full-image pass. Per-row work is proportional to span endpoints and covered runs, not
to the panel width; the per-pixel work charges are gone, so `max_work` now bounds sorting, spans,
span endpoints and morphology. A unit test compares 300 random bodies and cuts (every supported
sampling and several grayscale levels) against the previous dense per-pixel accumulation.

| Case | Sequential | Parallel layers | + parallel encoding | + span rasterization |
| --- | --- | --- | --- | --- |
| 20 mm cube, 400 layers at 13312 × 5120 (GOO) | 164 s | 36 s | 20.5 s | 8.4 s (1.47 GB peak) |
| FX-11, 2 mm hollow and 2 drain holes, 1000 × 600 | 12.5 s | 4.5 s | 4.1 s | 3.7 s |

Every column produces byte-identical files. A 1200-layer job at 13312 × 5120 now extrapolates to
about 25 s.

Encoding tests (`tests/print_export.rs`):
- byte-identical files, output size and coverage volume against `export_machine` for CTB, GOO,
  PP1 and Photon under both pixel policies, with 1, 2 and 8 threads and a one-layer window;
- the same failures for a source error, too few and too many layers;
- cancellation from the workers' predicate publishes nothing.

Tests (`tests/raster_parallel.rs`):
- identical images to the engine plus `RasterStream` for 1, 2, 3 and 8 threads, and with a
  one-layer window;
- identical cut images to `DifferenceRasterStream`;
- the same first failure as the sequential stream (layer 20 of 100);
- cancellation delivers only the layers that precede the first failure;
- an early consumer stop joins the workers.

## Decisions

1. **CLI default fill:** `union`, decided by the owner and applied after U1–U6 passed.
2. **Thread count:** all available cores (owner decision); no `--threads` option yet.
3. **Open-mesh policy:** still open; open sections fail explicitly.
4. **Window budget:** 1 GiB for layers and 512 MiB for encoding, as implemented.
