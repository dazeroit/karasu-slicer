# M5.5-B bounded convex inward hollowing

## Evidence and supported domain

This increment implements actual hollowed geometry in safe native Rust, following reference
behaviour and independent design. No other program's code, assets, hidden clamps, foreign geometry engine or new dependency is imported.

The initial backend is `convex-halfspaces-v1`. Input is one nonempty embedded, closed, outward,
consistently triangulated convex polyhedral solid. Positive nonuniform placement precedes world-mm
hollowing; transformed represented coordinates are authoritative. Coordinates outside +/-10000mm,
nonconvex solids, multiple/nested components, open/nonmanifold/degenerate/coincident/overlapping
surfaces or unresolvable constructions fail explicitly. This is not arbitrary-STL hollowing.
Analytic sphere comparisons separately account for a convex tessellation's source approximation.

The plan must contain exactly one hollow operation. Holes and supports remain C/D, not silently
ignored. The original-byte SHA is still a caller assertion: this module consumes an already decoded
mesh and cannot prove its correspondence with external original bytes. The asset owner must verify
that binding. Original mesh buffers are never modified. Exterior vertex/face order is retained after
the canonical placement. Interior geometry is independently derived and reversed in the final shell.

## Explicit revision of the provisional A backend

A provisionally selected implicit distance/cell reconstruction, contingent on solid-sign and topology
proofs. Those proofs are not supplied by the reference voxel/chamfer code. For B's first bounded
backend choose convex halfspace erosion: it implements the same Euclidean inward-distance material
for the supported convex domain without an unproved sign oracle, grid topology or foreign kernel.
This supersedes the initial voxel reconstruction choice for this backend only; general nonconvex
preparation remains future scoped work. A's plan/history/schema remains compatible. The result pins
its backend identity and exact plan record separately. Cell size/cell count remain recorded voxel
requests; this backend constructs zero cells and does not interpret cell pitch as achieved wall error.
The requested distance_error_mm is a strict upper budget on the certified erosion-parameter error.

## Mathematics and proof boundary

For convex K with outward unit support planes n_i·(x-a_i)<=0, the exact cavity at t is
K_t=intersection_i {n_i·(x-a_i)<=-t}. Erosion distributes over intersection, and a ball radius t fits
inside one halfspace precisely under this inequality. Hence this is Euclidean inward erosion, not
an independent face-translation heuristic or nearest-normal sign classifier.

The output convex cavity Q is accepted only with an inclusion certificate
`K_(t+delta) subset Q subset K_(t-delta)`, delta<=requested error and delta<t. A separately tested
point lies strictly inside K_(t+delta), proving the lower bracket is nonempty. The certificate bounds
erosion parameter; it is not a Hausdorff bound, especially near collapse, and does not imply exact
thickness at all corners. A source cube20,t2 has cavity16 and shell volume3904mm3 under this contract.

For the upper inclusion, interval-check every Qvertex against every original support plane at
thickness t-delta. Convexity extends this to all of Q. For the lower inclusion, each outward Qfacet
has a certified unit normal m and offset h about reference O. Choose an original support plane n,a.
On the original source bounding box, |(m-n)·(x-O)| is bounded by sum |m_j-n_j|R_j. An outward interval
upper bound on `n·(a-O)-t-h + sum |m_j-n_j|R_j` supplies sufficient delta for that facet. The minimum
over candidate original planes is valid. When one source plane is too conservative, use a nonnegative combination of up to three source
planes. Approximate Cramer weights from up to eight nearby support planes are only proposals:
all finite weights are clamped nonnegative and an interval residual `M-sum(lambda_i*N_i)` is retained.
The sufficient delta is `(sum(lambda_i*offset_i)-h+residualBBoxBound)/sum(lambda_i)`, with outward
interval arithmetic and a strictly positive enclosed weight sum. Any such weights prove the bound;
no exact normal-cone membership or exact floating solve is assumed. Checking every facet and proving
Q a closed embedded convex boundary gives the lower inclusion. No small residual alone is claimed to prove a surface-distance
bound. Source/cavity/material volumes use independent outward determinant-sum intervals.

## Exact admission and numerical construction

1. Validate recipe, caps and canonical placed coordinates; run M2 combinatorial topology with explicit
   input bounds. Require one closed consistently wound component without excluded or isolated records.
2. Use exact represented-coordinate orient3d to prove every vertex is behind each outward face;
   require a strict interior side. Reject distinct indices with equal coordinates. Verify pairwise
   geometric contacts: because both faces support the convex solid, any contact lies on zero-side
   vertices/edges. Exact projected triangle tests reject unshared contacts and coplanar crossings.
   Exact coplanarity deduplicates support planes, preserving first occurrence order.
3. Build interval unit face normals. Basic arithmetic widens each IEEE754 operation by one adjacent
   representable step. Root bounds are accepted only when exact integer squares verify them.
   Exact orient3d uses an interval filter and a fixed200x32-bit signed-integer fallback: finite
   binary64 differences need at most2099 bits, triple determinants below6301bits fit6400. No heap,
   truncation, unexplained epsilon or unbounded numeric dependency enters sign decisions.
4. Construct a *proposal* by clipping indexed polygon faces against approximate inward planes,
   sharing edge intersections and constructing projected exact convex cap cycles. Construction signs
   are rounded proposal decisions, not geometric truth. Empty/collapsed/range failures are explicit.
5. Snap only derived proposal coordinates to a declared numerical dyadic pitch: largest power of two
   <=requestedError/64, round ties away from zero, coordinate/pitch<=2^50. This is an explicit cavity
   construction approximation, not source welding or a physical printer grid. Build an exact-sign
   incremental convex hull of these candidates, with one checked oriented horizon cycle per insertion.
   Exact duplicate/interior candidate removal is allowed; the source is unchanged.
6. Prove the final hull's closed embedded convex geometry with the same independent admission checks,
   then apply the interval inclusion/volume certificate. If candidate rounding, facet drift or thin
   geometry cannot meet the budget, return Uncertain rather than a fabricated successful shell.
7. Append the reversed cavity to the placed original exterior, checking indices and budgets; publish
   only after final cancellation. No source repair, t/quality clamp or fallback-to-solid success exists.

Source-order repeatability is guaranteed for identical represented inputs/settings/build arithmetic;
triangle permutations need not produce byte-identical proposals. Exact hull topology is independent
of rounded visibility decisions. No geometry is certified before the final validation/certificate.

## Limits, failure, progress and ownership

Defaults:2048 input vertices,4096 input triangles,512 unique source support planes,20000 construction/
output vertices,40000 construction/output faces or triangles,50000000 charged parent traversal/
predicate units and256MiB cumulative requested capacity-increment admission. Each cap is independent; an
input below individual count caps can still exhaust work, memory or accuracy and fail. Caller-selected
caps never authorize a foreign kernel or numerical ambiguity. Output limits also apply to the final
combined shell. Default caller error must be positive and less than t/4; no requested accuracy is
silently widened. A smaller error can require more numerical range than representable and fail.

The work ledger charges parent traversal and predicates, not exact CPU instructions or subordinate
M2 topology internals; M2 has independent bounded input/cancellation. Standard sorts, allocation,
bulk copies, Vec/Arc destruction and callback work are internally indivisible. Cumulative requested
Vec capacity increments are admitted before allocation; this is not total allocator traffic,
reallocation copying or resident memory. M2 analysis receives a conservative4096*(V+F)-byte
allowance for its bounded linear incidence/link/component storage; this is an admission allowance,
not a measured allocator counter. Allocator metadata, caller/source residency and stack are excluded.
It is deliberately different from peak RSS and may reject a workload that would fit in live memory.

Errors distinguish InvalidInput with triangle context, Unsupported domain/recipe, Resource count/
work/storage/allocation, Uncertain numeric/construction/certificate, original mesh/topology causes,
and Cancelled. Cancellation checks entry, bounded traversal, certificate and publication, plus inherited
M2 checkpoints. No successful partial result escapes. Prior plans/history/source remain owned by the
caller and unchanged on any failure.

`hollow_mesh_with_progress` emits deterministic advisory phases Source,Construction,Reconstruction,
Certification,Assembly,Published with completed-stage counts0..5/total5. This is distinct from
cancellation checkpoints. Callbacks run synchronously, must return promptly and must not panic.
Published follows the final cancellation check; later requests need not prevent return.

## Result and independent acceptance

`HollowedModel` owns a sliceable f64 shell mesh, outward cavity preview mesh, the exact asserted plan
record, backend ID, inclusion/volume certificate and charged work/requested bytes. It is tied to the
same immutable plan; history generation admission remains the owner's responsibility. It certifies
geometry in this domain, not watertight physical prints, wall strength, drainage or exposure settings.

Acceptance fixtures cover analytic cube, nonuniform world scaling, arbitrary rotation/translation,
regular tessellated sphere with polyhedral/source-error separation, collapse/excessive t, invalid/
concave/multiple source rejection, unsupported operations, all cancellation phases, progress and
independent resource boundaries. M3 integration must yield the shell's outer region plus cavity hole.
Independent readers check actual serialized f64 geometry, exact represented volumes and topology;
no proprietary output is a correctness oracle. Native Linux and Windows quality gates are separate.
The [profiling protocol](../tools/m55b-validate/PROFILING.md) declares fixture budgets before timing.

Background: convex supporting-halfspace representation in
[Brondsted's Introduction to Convex Polytopes](https://users.fmf.uni-lj.si/lavric/brondsted.pdf),
and the distinction between [exact predicates and constructions](https://www.cgal.org/exact.html).
These are mathematical references only; no CGAL kernel, code or dependency is used.
