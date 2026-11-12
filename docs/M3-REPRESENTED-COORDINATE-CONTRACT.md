# M3 Represented-Coordinate Contract

## Requirement and separation

The owner authorized the remaining-M3 work and implementation on 2026-10-17. It addresses a
concrete numerical bottleneck:
ordinary approximate triangle-plane intersections need not land on any caller-selected TD-025
integer lattice. Passing them through mandatory `ExactOnly` conversion prevents usable layers.

TD-031 adds an explicit grid-independent path. Existing APIs and default engine behavior retain
TD-025 failures; callers choose `LayerEngine::with_represented_coordinates()` when they want
millimeter contours without integer conversion. This makes the choice reviewable and preserves
callers that intentionally require an exact grid. No physical precision default is selected.

## Strict topology

`retain_represented_vertices_with_cancellation` borrows the unchanged inspection, validates finite
agreed coordinates and emits `GridVertexOutcome::Represented` for each agreed vertex. Conflicts
remain unresolved. `GridSummary` separately counts represented, exact-grid and unresolved vertices;
represented mode performs zero axis conversions. Vertex/payload caps and existing cancellation
checkpoints apply. The axis-conversion cap is inactive because no conversion occurs.

`ExactGridView` is the compatibility name for the shared coordinate-view container. Its
`uses_represented_coordinates()` flag is authoritative, including empty/all-conflict views.
`spec()` retains inactive caller metadata in represented mode; it does not describe coordinates.
The owned `RegionLayer` exposes the same flag, and `grid_spec()` is inactive in that mode.
No `Exact` index pair is fabricated. Original `CoordinateAgreement` supplies millimeter geometry.

The strict forest, eligibility policy, source path IDs, fill rule, work/capacity budgets and contact
reporting remain shared. Exact-grid integers are exactly representable in binary64 under TD-025's
2^52 bound. Both input modes use binary64 comparisons for bounds/half-open Y intervals and the
existing exact dyadic orientation predicate for crossing signs. No division, global epsilon,
welding, coordinate movement, topology repair or rational-to-float conversion is introduced.
Strictness is about represented coordinates after the reviewed approximate section kernel, not
ideal real-valued intersections. Open, ambiguous, self-contacting or conflicting source geometry
still cannot claim resolved material.

## Lossless constructive composition

`RepresentedMaterialRing` and `union_represented_material_rings[_with_cancellation]` accept finite
binary64 XY millimeters. Decompose each nonzero coordinate into a signed odd significand and an
integer binary exponent, including subnormals. Select the minimum exponent and shift significands
into common integer units without floating subtraction, scaling, multiplication or rounding.
Zero has no scale requirement; signed zero denotes the same exact coordinate. All-zero/empty
input uses exponent zero. Every encoded coordinate must lie within +/-2^60. Wider dynamic ranges
return `InvalidRing { path, reason: DyadicRange }`; NaN/infinity return `NonFiniteCoordinate`.
No fallback snaps or silently drops input.

The existing arrangement constructor receives these integers through a private reviewed 60-bit
entry. The public integer-ring API keeps its +/-2^52 domain. Result rational coordinates carry
`ConstructedMaterial::dyadic_exponent_mm() = Some(e)`; physical XY is the exact rational multiplied
by 2^e millimeters. `None` means caller-defined integer units under the old API. This scale is not
a raster pitch or an approximation budget. Consumers must retain it rather than interpret encoded
integers as millimeters. Source labels, parity/union semantics and complete atom provenance remain
TD-030's. The owned-layer adapter uses original agreed XY and attaches output only before atomic
publication. Strict diagnostics/completeness remain independently visible.

For B=2^60, original differences are <=2B. Intersection numerators have magnitude <=24B^3 and
positive denominators <=8B^2. Exact point comparison is below 384B^5; symbolic midpoint numerators
are <=384B^5 and denominators <=128B^4. Comparing a midpoint with an original-supported rational
line needs at most 24576B^8 < 2^495. Eight 64-bit magnitude limbs therefore suffice; original integer
orientation/angular products are <8B^2 <2^123 and fit signed i128. Other fixed arithmetic remains
checked. No generic rational-area expansion or intersection of successively constructed segments
is introduced. The 60-bit extreme regression exercises rational topology with exponent zero.

Encoding uses O(V+R) scratch/work before the existing bounded arrangement. Both decoding passes
and ring records charge the shared work budget; scratch is charged before fallible reservation
and deducted from the constructor's remaining budget. The constructor summary includes encoding
work and requested capacity. Cancellation checks every encoding vertex/ring and reservation phase;
publication remains atomic. Adapter scratch is independently charged against the same remaining
payload cap. Requested capacity is not a process-RSS or cancellation-latency guarantee.

## Scope and acceptance

The identified mandatory-grid implementation gap is closed for strict represented-coordinate
regions, and constructive composition accepts ordinary decimal/quarter-plane geometry within its
explicit exact dynamic-range domain. Arbitrary finite-f64 dynamic range is not promised by the
bounded constructor. Rejecting an explicit unsupported range is distinct from silently quantizing
or claiming that every valid mesh can be processed under every resource budget.

Self-contact repair, inferred source ownership, model preparation, offsets, hollowing, rasterization,
general Boolean command families and parallel/spatial frameworks are not added. No further
mandatory missing M3 module remains under the existing conservative geometry contract. Whole-M3
acceptance still needs current native Windows replay, agreed-corpus review, explicit preliminary
large-input budgets and retained reference research gates. Static upstream inspection and short
Linux campaigns do not fulfill those acceptance gates.
