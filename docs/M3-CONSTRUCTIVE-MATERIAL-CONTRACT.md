# M3 Constructive Material Contract

## Scope and authorization

The owner's 2026-10-17 request authorizes auditing and filling remaining M3 implementation gaps
after reference research. TD-030 resolves P-M3-028 for explicit constructive material
composition. No reference program's observed behaviour establishes a safe implicit ownership,
snapping or cleanup rule. Existing strict APIs and their outcomes remain valid.

The added operation accepts individually simple closed integer rings in the TD-025 domain
abs(x/y)<=2^52. Each ring has an explicit material-source ID and unique source-path ID. Material is
even-odd within one source and union across sources. Thus a nested ring from the same source can
form a cavity while a nested independently filled source remains filled. Winding/connectivity do
not infer ownership. Empty input denotes empty material. Repeated closing vertices, collapsed
edges, self-contact/intersection, invalid ranges and duplicate source-path IDs are rejected with
structured context. Cross-ring proper intersections, endpoint contacts, collinear overlaps and
coincident rings are supported; no input is silently repaired or discarded.

## Independent exact arrangement

Intersect original integer segments only, split them at proper intersections, endpoint contacts and
overlap endpoints, and group coincident atomic geometric segments while retaining every source
path/edge support. Exact homogeneous vertices (X,Y,W), W>0, need no gcd reduction for mathematical
comparison. No generated rational edge is re-intersected and no rational area sum is required.

For every atom, classify its left and right infinitesimal neighborhoods using the exact rational
midpoint plus epsilon times an original integer normal. Ray tests compare the constant term first,
then the epsilon coefficient; epsilon is symbolic, not a physical offset/tolerance. Use half-open
Y intervals. Retain a directed atom exactly when union material differs across it, oriented with
material on the left. Coincident supports are explicit provenance, never an unexamined parity count.

Traverse boundary atoms by the clockwise successor from the reverse incoming direction at each
vertex. Original integer supporting directions suffice for exact angular ordering. Split a walk at
repeated vertices into edge-disjoint simple cycles, retaining contact vertices and all atoms. This
represents point-touching regions without falsely claiming strict non-contacting boundaries. Orient
cycles from the turn at their lexicographically minimum geometric vertex; do not accumulate rational
areas. A symbolic geometric-interior probe determines containing cycles and the immediate-parent
forest. Even-depth counterclockwise cycles are outers; odd-depth clockwise cycles are holes.
Contacts can be shared across cycles. Hierarchy contradicting material-left orientation is a typed
topology failure, never a fabricated region. No strict-region record is reused to imply contacts
have disappeared. Output explicitly exposes its constructed vertices/edges/cycles/regions.

## Arithmetic domain and representation

For B=2^52, integer differences are <=2B; original-line intersection homogeneous numerators are
bounded conservatively by 24B^3 and positive denominator by 8B^2 (roughly 161/108 bits). Point
equality/order products stay below 270 bits. Midpoint numerators/denominators stay below 271/218
bits. Symbolic Y comparisons and original supporting-line predicates fit within 512 signed
magnitude bits; constructed-loop predicates use original supporting lines rather than an expanded
rational determinant. Exact angular products use i128 under the original difference bound.

Eight-u64-limb checked signed magnitude arithmetic is independently implemented in safe Rust.
Private-constructor point comparison has a proved capacity invariant, covered at domain extremes.
Other overflow returns a typed error. Public exact-coordinate getters export sign/magnitude limbs
and common positive denominator; representation is deterministic for fixed input but not necessarily
gcd-reduced. Equality/order mean rational equality/order. No f64 decoder, grid snapping, rounded
intersection or positive physical error budget is introduced. Output remains in explicit grid units;
the layer adapter retains its existing validated BinaryGridSpec to interpret millimeters.

## Limits, determinism and cancellation

Explicit caps cover input edges, unordered input pairs, exact vertices, atomic segments, total
charged primitive work and aggregate requested vector capacity. Checked count/byte/isize arithmetic
and fallible reservations precede retention. Dynamic capacity growth charges requested capacity
increments; temporary and output buffers share the constructor budget, without claiming hard RSS,
allocator overhead, elapsed-time bounds or worst-case performance. Inputs are immutable.

Original input order, exact point comparisons, source-edge order, canonical atom endpoints and
canonical cycle start/direction define reproducible output. Source permutation is assessed through
geometric/provenance equivalence, not identical source IDs or homogeneous limb encodings.
Entry, input/pair/split/classification/topology phases, bounded inner work, both reservation sides
and final publication poll TD-014 cancellation. No successful partial constructed material escapes.
Sorting, fixed arithmetic, allocation and destruction remain indivisible. No dependency or
parallel/spatial framework is added. The initial all-pairs arrangement is deliberately bounded;
profiling precedes acceleration.

## Layer integration

The standalone constructor consumes explicit rings. The optional M3.6 composition adapter instead
requires one material-source ID per original source face. Every eligible cycle must have a unique
agreed source ID across all its raw supporting face occurrences. Mixed ownership, open/ambiguous
paths, non-simple paths and unresolved complete-only groups are rejected, preserving the original
layer for inspection. Geometric cross-path contacts alone are admitted and resolved by construction.
Tangent-only nonmaterial context remains retained. No material identity is inferred from a name,
orientation, nesting or source connectivity.

LayerEngine's explicit opt-in union mode retains its existing owned strict result and attaches the
constructed result before final layer publication, with the same cancellation/atomicity/fusion
contract and an additional checked owned payload charge. Existing LayerConfig literals/default
strict behavior remain compatible. Strict completeness/diagnostics are not relabeled; constructed
material is a separate successful result. M5.5 will provide explicit model/support source semantics.

## Validation and closure

Independent analytic occupancy, exact small-rational intersections, material-source parity/union,
overlap/coincidence/contact and nested-hole fixtures must verify output boundaries and hierarchy.
Include the TD-025 >i128 construction counterexample, full coordinate bounds, deterministic reruns,
source/provenance conservation, every cap/cancellation phase and error atomicity. Add bounded fuzz
and a dedicated strict-region corpus. Record representative timing/requested bytes/peak RSS without
inventing performance thresholds. Native Windows validation and remaining EXP-03/04 research are
separate gates; this implementation does not mark whole M3 complete by assertion.
