# M5.5-C finite drainage cuts

## Requirements and supported domain

Follow reference research, behavioral requirements, independent design, safe-Rust implementation,
and validation. Reference research records the behaviour of other programs. Runtime connectivity,
overlap and clearance guarantees are unknown; the following conditions are our own requirements.

The initial backend evaluates one certified M5.5-B convex hollow solid followed by finite cylindrical
cuts parallel to positive/negative world X, Y or Z. World placement precedes cuts. A raw cardinal
axis may have any nonzero finite magnitude; only its orientation matters. Oblique axes, through cuts
that terminate outside the cavity, supports and generic Boolean mesh export are unsupported.
Arbitrary STL preparation remains unsupported by the upstream hollow backend. These restrictions
are explicit errors, not invisible clamps, repairs or claims of universal preparation support.

Plans persist exact entry, direction, length, diameter and operation IDs. Edit/remove means creating
and evaluating a revised immutable plan; existing results keep their original canonical bytes.
Zero cuts are valid and report one enclosed cavity. One accepted cut provides geometric access to
the single connected convex cavity and reports zero enclosed cavities. Physical resin evacuation,
venting, suction behavior and structural strength are outside this geometric statement.

## Equivalent construction and slicing

The prepared solid is `K \ (Q union C1 union ... union Cn)`, where K is the placed convex exterior,
Q the certified represented cavity and Ci finite polygonal cylinder approximations. No Boolean
triangle mesh is fabricated. `DrainedModel` owns a private boundary scene and explicit face-source
labels; only its exact material-difference layer evaluator represents the drained solid. Its
`hollowed_model()` getter is deliberately the uncut hollowing result for comparison/preview.

Source zero uses even-odd parity for the original shell; each nonzero source has its own parity,
and their union is subtracted. The supporting general exact integer/dyadic 2D APIs also handle
overlapping cutter rings. The drainage admission policy is more conservative and rejects cutters
without a certified separating coordinate axis between their bounding boxes, including touch or
uncertain proximity. No overlap is silently merged, discarded or reported as validated separation.

Layer generation uses existing M3 triangle-section ownership and represented-coordinate provenance.
Floating section interpolation retains the existing M3 contract; exact planar construction means
exact for those represented section coordinates, not exact-real intersections of original planes.
Exact coplanar cutter-cap layers use a narrowly verified mixed cap/side perimeter rule (TD-039):
exactly two coherent opposite-wound, distinct, nondegenerate source-face occurrences retain both
records and one patch-boundary candidate. Extra or inconsistent incidence remains unresolved.
The existing exact rational arrangement splits contacts/intersections without rounding them to a
physical grid. Body membership is parity of source zero AND NOT union of cutter parities. Existing
union APIs retain their original semantics. RegionLayer's compatibility accessor `material_union()`
contains the selected constructed composition, whether union or difference; strict diagnostics remain
separate. M4 rasterization consumes that explicit constructed material.

## Polygonal cylinder certificate

Generate candidate circular rings by ordinary sine/cosine, starting with eight sides and doubling
up to the caller cap (hard maximum 4096). Trigonometric output is a proposal, not an accuracy proof.
For actual represented world coordinates, outward intervals independently certify:

- Ordered quadrants, positive consecutive radial determinants, and positive consecutive edge turns
  establish one simple strictly convex winding about the represented center.
- Every vertex radius is at most Rmax; every oriented edge's support distance from the center is at
  least Rmin. Therefore the centered disk of radius Rmin is inside the polygon, which is inside the
  disk of radius Rmax.
- Radial deviation is bounded against interval diameter/2; finite extrusion endpoint rounding has
  an independent axial bound. Their outward-rounded sum must not exceed the requested plan distance
  error. Failure to meet it at the segment cap is explicit, never an automatic tolerance increase.

Reversing winding for a negative axis produces the same outward finite-prism convention. Endpoints
use the same represented radial coordinates, so both caps are planar and matching. Side quads split
consistently and cap fans produce closed indexed cutter meshes. The interval implementation is
shared with B; its exact integer square-root verification is retained. No new dependency is added.

## Connectivity and clearance proof

Require all entry-cap vertices strictly beyond one common exterior supporting plane with at least
`minimum_clearance_mm`. Require all end-cap vertices strictly inside every represented cavity
supporting halfspace with at least that margin. Compute distances with outward interval normals
and arithmetic; failure or numerical ambiguity cannot publish a connected result.

A convex polygon cap satisfies every linear inequality satisfied by its vertices. Every generator
of the straight prism starts outside the exterior and ends inside the convex cavity. Convexity
therefore gives a positive-area passage through the shell into that cavity; the terminal cap cannot
cut an unrequested opposite wall. The cylinder approximation error describes the geometric cut,
not an exact curved surface or physical wall guarantee.

Failures preserve stable operation IDs. Categories distinguish entry not certified exterior,
end center not certified within the model, center lacking cavity entry, and insufficient whole-cap
clearance. Near-boundary cases are rejected conservatively. Potential overlap reports both IDs.
Underlying plan, hollow, mesh, cancellation and numerical causes remain structured and chained.

## Budgets, cancellation and integration boundary

Default drainage policy: 32 cuts, at most 4096 segments each, 50 million charged containment/assembly
units, 256 MiB aggregate admission, and an explicit 1e-6 mm strict clearance. This clearance is an
admission margin, not a global epsilon, welding distance or automatic repair policy. Plan sampling
error controls radial/axial approximation independently. Finer tolerances may exhaust segment or
per-layer arrangement limits even after successful scene admission; scene construction is not a
promise that every subsequent layer fits a caller's independent slicing/raster budgets.

Before hollowing, reserve conservative allowances: each cut gets 1024*max_segments bytes plus 4096
bytes of metadata; shell-copy buffers get 24*max_output_vertices+20*max_output_triangles bytes; the
canonical plan is charged separately. Hollowing receives the remaining payload admission. These
are conservative capacity allowances, not total allocator traffic, copying overhead or process RSS.
Each cylinder is precharged 100*max_segments traversal units; containment, overlap and assembly
loops charge individual records. Hollowing, sectioning, exact arrangements and rasterization retain
independent limits. Checked arithmetic and fallible vector reservations reject overflow/exhaustion.

DrainageStart, bounded DrainageWork, and DrainagePublish checkpoints guarantee atomic scene
publication. A later layer stream may expose completed layers before a subsequent failure under
its existing fused stream contract; consumers needing whole-job atomicity must collect/stage output.
Sorts, allocation/copy/destruction and interval primitives are indivisible work units, not elapsed
latency guarantees. No GUI or new CLI preparation command is added before M6; public headless APIs
and owned reference fixtures provide reviewable use and validation.

Candidate trigonometric values are host-library proposals; the independent interval certificate
validates the represented cutter on each platform. Repeatability is required on the same build;
cross-platform mask equality is checked on the owned fixtures and is not a universal bitwise
geometry guarantee for every trigonometric input. Terminal clearance and cutter separation also
do not constitute a remaining-wall-ligament or structural thickness certificate near openings.
