# M3.5-A Strict Region Hierarchy Contract

## Status and scope

This contract defines the implemented M3.5-A `EvenOddStrict` hierarchy. It is a nonconstructive,
single-plane analysis over existing immutable M3.4-A/B/C reports. It does not schedule planes,
construct intersections, union source components, close paths, weld coordinates, repair geometry,
rasterize, select a printer setting, or claim printability.

`slicing::regions::{assemble_strict_regions,
assemble_strict_regions_with_cancellation}` accepts one `PlanarInspection`, its matching
`SectionAreas`, and its matching `ExactGridView`. Pointer identity is checked: reports borrowed
from different inspections are a typed `RegionError::InvalidInput` failure.

## Strict boundary eligibility

Exactly one `PathRegionOutcome` is emitted for every original path ID. A path becomes a strict
boundary only when all of the following hold:

1. its M3.3 kind is `ClosedCycleCandidate`;
2. its M3.4-A embedding is `SimpleCycle`;
3. its M3.4-B outcome is `Measured`, which includes a nonzero exact orientation;
4. every referenced graph vertex is `GridVertexOutcome::Exact` in the matching TD-025 view; and
5. no retained cross-path planar contact involves the path.

Open chains, ambiguous trails, non-simple cycles, unresolved coordinates, and contact-involved
cycles remain explicit `NonBoundaryReason` values. No path is deleted or automatically closed.
The original inspection retains raw tangent, internal coplanar, unresolved complete-only and source
provenance records.

## Exact containment

The implementation stores eligible vertices as their existing TD-025 integer XY pairs. It first
rejects impossible containment using inclusive integer AABBs. For remaining unordered pairs it tests
the first canonical vertex of each cycle against the other cycle with a positive-X ray. An edge is
counted by the half-open Y rule: exactly one endpoint must be at or below the query Y while the
other is strictly above it.

Side comparisons use

```text
(bx - ax) * (py - ay) - (by - ay) * (px - ax)
```

in signed `i128`. TD-025's inclusive `|q| <= 2^52` bound limits coordinate differences to `2^53`,
so the exact product/subtraction domain fits `i128`; no division, floating arithmetic or epsilon is
used. M3.4-A has already excluded boundary contact for strict candidates. If pair classification
would nevertheless say that both cycles contain one another, assembly returns a typed consistency
error rather than choosing an arbitrary order.

## Forest and material interpretation

Every strict containment relationship is retained before deriving depths. A cycle depth equals its
number of strict containers; its immediate parent is the unique container at depth minus one. A
missing or non-laminar parent is a typed invariant failure. Rounded area magnitudes are never used
to choose containment or parents.

The only fill rule is `RegionFillRule::EvenOddStrict`:

- even-depth boundaries are `Outer` boundaries and each starts one `MaterialRegion`;
- direct odd-depth children are `Hole` boundaries of that region;
- an even-depth island inside a hole starts a separate region; and
- depth-zero roots are disconnected regions.

Boundaries remain in original path-ID order. Regions are ordered by their outer boundary path ID;
each region's holes are ordered by path ID. Existing traversal orientation is recorded but does not
decide material. `reversed_view` is true exactly when a caller must reverse the canonical borrowed
path to view an outer counterclockwise or hole clockwise from positive Z.

## Completeness, limits, errors and cancellation

`RegionCompleteness::Complete` means all retained context has a reviewed boundary or non-material
interpretation. `GeometricallyUnresolved` is a successful result when open/ambiguous/non-simple or
contacting paths, grid coordinate conflicts, or unresolved complete-only groups remain. It is not a
printability certificate. Cross-path contacts and global unresolved context also appear in stable
`RegionDiagnostic` records; detailed upstream diagnostics stay borrowed.

`RegionLimits` independently caps paths, candidate-cycle vertices, pair classifications, ray-edge
tests, retained containment relations, boundaries, regions, diagnostics and aggregate requested
vector capacity. The direct point-in-cycle implementation is bounded by its explicit ray-test cap;
its work is O(V + P + R) for V eligible vertex references, P tested ray edges and R retained
relations, with up to O(C^2) unordered cycle pairs. No aggregate RSS or latency bound is implied.

TD-014 checkpoints are `RegionsStart`, `RegionsInput`, `RegionsStorage`, `RegionsBounds`,
`RegionsPairs`, `RegionsForest`, `RegionsOutput`, and `RegionsComplete`. Storage checks surround
every nonempty fallible reservation. Cancellation, cap, allocation and invariant errors publish no
partial `SectionRegions`; geometric ambiguity is result data instead.

## Deferred work

M3.5-B owns explicit layer scheduling. Crossing/touching/coincident overlay construction, source
material union semantics and constructed-vertex provenance remain a separate M3.5-C decision gate.
M3.6 owns a self-contained `RegionLayer` and multi-layer orchestration. Native Windows validation,
expanded fuzzing and performance baselines remain acceptance work for this implementation increment.
