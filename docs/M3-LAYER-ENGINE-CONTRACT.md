# M3.6 Owned Layer Engine Contract

**Subsequent extension:** TD-030 adds an explicit opt-in face-material union mode under the
[constructive contract](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md). It retains this strict result and
attaches a separate exact constructed result before atomic publication; strict default semantics
and existing configuration literals remain unchanged.

## Authority and scope

The owner authorized reference research, independent specification and M3.6 implementation on
2026-10-17. TD-029 accepts this contract (P-M3-028 remains reserved for the separate overlay
proposal). M3.5-C constructive overlay remains independently gated;
M3.6 composes the existing strict algorithms without union, repair, rounding or new material rules.
This is not rasterization, print-job publication, printer selection or whole-M3 acceptance.

## Inputs and execution

`slicing::engine::LayerEngine` borrows an immutable M2 mesh, explicit source-to-world transform
and validated M3.5-B `LayerSchedule`. `LayerConfig` names the validated exact binary XY grid,
`EvenOddStrict` fill rule, all eight stage limits and an owned-output byte cap. Geometry uses each
published finite schedule Z unchanged. No plane adjustment, mesh mutation, snapping or inferred
physical policy occurs. Stage-limit defaults are resource policies, not slicing defaults.

Each iteration executes the existing cancellation-aware section -> normalization -> graph -> paths
-> planar inspection -> area -> exact-grid -> strict-regions pipeline. All mathematical contracts
remain unchanged. Off-grid input is a fatal grid error; open/contacting/ambiguous geometry remains
structured successful `GeometricallyUnresolved` data, never fabricated complete material.

## Owned output and provenance

Each successful item is an immutable, independent `RegionLayer`. Temporary borrowed reports never
escape. Output retains the original raw event stream; endpoint observations and segment occurrences;
source-feature vertices with coordinate agreement, exact-grid outcomes and degrees; every complete
graph edge with original multiplicity/ownership/issues and disposition; every canonical path with
vertex/edge references, embedding, area and region disposition; components; strict boundaries,
material regions and hole references; graph/path/planar/region diagnostics and stage summaries.
IDs and order are unchanged. Raw event/source-face/triangle/local-origin chains remain resolvable.
Keep the immutable source asset when interpreting its IDs; no persistent asset identity is invented.

Derived adjacency and coordinate-sorted coincidence lookup buffers are not copied: they contain no
unique geometry or diagnostic evidence, and their source-feature/coordinate/occurrence data remain
owned. Paths use canonical traversal with no duplicate closing vertex; `reversed_view` remains
explicit boundary metadata. No coordinate orientation rewrite or ring construction occurs.
Flat buffers avoid self-referential structures and per-path heap allocation. Outputs outlive the
engine, schedule, source mesh and transformation values. They do not borrow cancellation/progress.

## Stream lifecycle and progress

Construction does no processing and produces no callbacks. `StreamState` is NotStarted, Running,
Complete or Failed. `next()` yields ascending contiguous schedule identities, one atomic layer at
a time. The first operational error or observed cancellation yields exactly one contextual error,
then the iterator is fused. Errors retain the scheduled plane when a layer was active and preserve
the underlying structured stage error/cancellation cause. Prior successful layers remain individually
complete, but do not form a complete stream/job until normal exhaustion.

Completion is observed only by advancing past the last scheduled plane. This performs a final
cancellation check, including empty schedules. Yielding the last layer does not claim stream
completion. Dropping an unexhausted engine does not generate a completion event. `size_hint` has
lower bound zero and a conservative upper bound allowing an error at final exhaustion.

An optional synchronous observer receives `LayerProgress` with stage `M3LayerGeneration`, kind
Started/LayerStarted/LayerCompleted/Completed/Failed, completed-layer count, exact total count and
current layer index when applicable. Completed counts never decrease; only successful layer
publication increments them. Failure never emits Completed. Callback panic/unwind and callbacks
with unbounded execution time are outside the contract; no panic catching, threads or async runtime
are introduced. Events describe orchestration progress, not source-face work, runtime estimates or
proof that geometry is resolved. The callback has no mutable geometry access.

## Cancellation and resources

The owned `Cancellation` controller borrows the caller's predicate; it is reused synchronously by
every stage. Existing TD-014 checkpoints remain unchanged. New checkpoints: LayersStart before any
stream work, LayerStart before each layer, LayerStorage on both sides of each nonempty owned-output
reservation, LayerOutput at bounded projection-record intervals, LayerComplete immediately before
layer publication, LayersComplete before successful exhaustion, CollectionStart before collector
preflight, CollectionStorage around a nonempty collection reservation, CollectionComplete before
atomic collection publication. Observed cancellation wins at its checkpoint; earlier errors retain
precedence. Allocations, fixed arithmetic, sorting and destruction remain indivisible.

Each stage independently enforces its reviewed work/storage caps. Projection checks all count/byte
arithmetic and isize capacities and its aggregate byte cap before reserving output buffers. Owned
payload counts the retained raw vector's actual element capacity and requested flat-output element
capacities. It excludes the RegionLayer header, allocator overhead/over-allocation beyond requests,
source/schedule storage, fixed scratch and temporary upstream reports. There is no total-RSS or
wall-clock guarantee: projection overlaps upstream reports until conversion completes.

No mesh cache, spatial index, parallelism, constructor or speculative job API is added. Time is the
sum of existing per-plane stages plus linear owned projection; memory is one working pipeline and
one output, excluding layers retained by the caller. Measure future budgets before acceleration.

## Atomic collection

`collect_layers(engine, CollectionLimits)` consumes only a fresh, unstarted engine. It preflights
the full known count, caller count cap and collection-header capacity before one fallible reservation.
Requested bytes equal count*size_of(RegionLayer) plus the sum of each layer's owned payload. This
is an independent aggregate cap; a generated layer may transiently exist before it is rejected by
the collection cap. Partial vectors are dropped on error. Any previously started engine is rejected
instead of returning a remainder mislabeled as a complete schedule.

Collector completion notification is deferred until final exhaustion, aggregate checks and
CollectionComplete succeed. Progress observes generated layers even if a later collection check
fails, but no Completed event follows that failure. A successful `LayerCollection` is complete for
its schedule, while each contained layer can still be geometrically unresolved. File/job atomicity
remains M5. Engine/collector errors include structured source, plane context and resource reason.

## Acceptance

Use independent analytic fixtures and direct-stage comparisons for clean/nested/disconnected,
empty/tangent/coplanar/open/ambiguous/contacting and off-grid cases. Verify owned provenance,
transformed-source preservation, output independence, flat-reference conservation, deterministic
fixed-width/bit encoding, all stage errors, resource and cancellation boundaries, fused termination,
normal versus failed completion and collector atomicity. Test empty schedules and failure after
previous successful layers. Native Windows, full-M3 fuzz/performance and preliminary budgets remain
separately reported gates; Linux/WSL results do not satisfy them by implication.
