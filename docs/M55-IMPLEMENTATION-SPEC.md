# M5.5-A preparation contracts and independent implementation design

## Scope and evidence

**Subsequent B implementation:** TD-038 selects a certified convex-halfspace backend for the first
bounded geometry domain, superseding the provisional voxel reconstruction choice below for that
backend. [B specification](M55B-HOLLOWING-SPEC.md) and [validation](M55B-VALIDATION.md) are authoritative
for implemented geometry; this A document retains its original design comparison and obligations.


This step implements validated immutable operation plans, exact-content surface bindings, canonical
replay records and bounded transactional in-memory history. It does not implement hollowing, Boolean
subtraction, support meshes, solid certification, scene evaluation or project-file loading. No public
API returns a prepared mesh or successful geometric validation. B/C/D implement geometry; E validates
its M3/M4/M5 integration. The earlier M5 Windows Release policy failures are retained separately; complete native passes
during this increment subsequently resolve its software gate without changing host policy.

Requirements derive from PREP-001–007 and the separately recorded reference behaviour.
Observations of other programs' states and controls are not evidence for undisclosed geometric algorithms or defaults. No reference implementation is copied.

## Implemented plan contract

A source binding contains an original-byte SHA-256 supplied by the asset owner and an M2
ObjectTransform. The digest is an assertion: this module neither reads original bytes nor verifies
that an externally supplied decoded mesh matches them. Asset loading must verify that relationship
before geometry evaluation. Empty or invalid meshes cannot become solids by supplying a digest.
Positive finite object scales are required; reflections and collapsed placements are unsupported in
this preparation contract although M2 can represent them. Rotation and nonuniform positive scale
are permitted. Placement is evaluated before all preparation operations, in world millimeters. The recorded flattened
Affine3 is authoritative for evaluation; raw ObjectTransform metadata must not select a differently
rounded direct transform path. Equivalent raw transforms with identical affine bytes share placement
identity.

Plans pin algorithm/schema version one, source binding, explicit sampling settings, stable nonzero
operation IDs and ordered full parameters. At most one hollow operation is permitted, preceding all
cuts. Cuts require hollowing. Manual supports follow all cuts. No automatic sorting, insertion,
repair or disabled-operation semantics are inferred. Remove/edit through complete validated snapshots;
there is no in-place mutable operation exposure. IDs are unique within a snapshot, not a global
persistent asset registry. Identical source/settings/order reproduce identical canonical records.

World-frame hollow thickness, finite cylindrical cut entry/direction/length/diameter and manual
support contact/base/dimensions are explicit. A cut's direction is normalized with scaled arithmetic;
its orientation is preserved. Raw direction values are retained and normalized only for checked
calculations, so rebuilding an admitted record cannot accumulate normalization drift. Support bases lie on world platform Z=0; contacts must be above it.
Tip radius cannot exceed shaft radius and shaft radius cannot exceed base radius. These are parameter
contracts, not collision, drainage or connection proofs. All coordinates/endpoints and lengths are
bounded by caller limits; subnormal/too-small features fail the explicit minimum-feature policy.
No global geometric epsilon is introduced.

A support surface key contains the exact canonical source/placement/sampling and hollow/cut prefix,
not a collision-prone short hash or pointer identity. Rebinding is explicit: build the edited model
prefix, obtain its key and deliberately supply that key with edited support contacts. Upstream edits
or placement changes reject old support keys; support-only edits do not invalidate other contacts.
World-positioned cuts remain fixed in world coordinates after placement edits; they are not surface
anchors, and later evaluation must diagnose misses. No automatic nearest-surface projection occurs.

Canonical records are a documented versioned binary replay identity, not a project persistence API:
ASCII `KPREP1`, source digest, f64 affine coefficients/translation, sampling scalars, model count,
ordered tagged model records, support count and tagged support records. IDs/counts use little-endian
u64; scalars use little-endian IEEE754 f64 with signed zero canonicalized. Raw direction is
recorded; scaled direction inputs intentionally have distinct content identities. Prefix keys include all model record content. Support records omit redundant prefix bytes
because validation proves their equality. External source bytes, paths, clocks and process-local
history generation do not enter the record. Versions must change if the interpretation changes.

History stores immutable snapshots and a cursor. Commit drops redo only after validation, resource
admission, allocation and the final cancellation checkpoint. Undo/redo restore content while
incrementing a monotonic process-local generation. Background consumers must pin both history owner
and generation/current record; generation equality across different histories means nothing. Undoing
and restoring identical content cannot accidentally revive an older background generation. This
module supplies matching checks, not a geometry publication or cache implementation.

## Resources, failure and cancellation

Default admission: 4096 operations, 8 MiB conservative per-plan retained payload, minimum feature
0.000001 mm, maximum absolute coordinate/length 1000000 mm. Sampling cell size must meet the same
feature/range limits; distance-query error must be positive and <=cell/32. A requested cell cap is
nonzero; it is recorded, not evidence that a grid has been constructed. History defaults: 64 snapshots
and 64 MiB conservative summed plan payload, counting shared payload repeatedly rather than
underestimating it. Future grid geometry has separate budgets.

Recoverable failures distinguish invalid fields/order/IDs, stale anchors, resource/allocation failure
and cancellation. Plan/history publication is transactional; cancellation and failure preserve source,
current snapshot, undo/redo and generation. Cooperative checkpoints are PreparationStart,
PreparationRecords and PreparationPublish, with loop polling every 256 records. Allocation, Arc
creation/destruction and standard fixed-size scalar arithmetic are indivisible. Requested payload is
not allocator RSS, stack use or a wall-clock guarantee. No threading framework is selected.

## Geometry algorithm review and B–E implementation specification

| Approach | Strength | Required proof/work | Decision |
| --- | --- | --- | --- |
| Direct offset/shell surface | Compact surfaces and smooth source fidelity | Concave intersections, offset singularities, thin-wall collapse and robust 3D arrangement | Not the initial backend; no robust kernel is currently admitted |
| Triangle Boolean surface arrangement | Exact boundary provenance in representable cases | Global solid validity, intersection construction, coincident faces and output manifoldness | No foreign kernel; independent implementation only if bounded implicit route cannot meet fixtures |
| Signed distance plus implicit CSG | One definition for inward shell, cylindrical holes and support union | Certified sign/distance queries and explicit uncertain states; nearest-face normals are insufficient | Chosen mathematical representation for B–D, contingent on oracle/admission tests |
| Bounded voxel/cell reconstruction | Deterministic bounded work, explicit resolution and connectivity | Discretization, thin features, ambiguous contacts and nonmanifold cell boundaries | Initial reconstruction design for B; no topology guarantee from spacing alone |

Define negative-inside world-space distance d to the validated placed source solid. Shell material is
`d<=0 AND d>=-t`. Subtract finite oriented capped cylinders for cuts, then union explicitly described
support tip/shaft/base primitives. Intended operation order is placement -> shell -> cuts -> supports;
cutting supports requires a future explicit operation contract. Do not use source-group XOR for
material union. Evaluate source/model/support material together before M3, rather than separately
exposing overlaps twice.

B admission must establish nonempty consistently oriented closed manifold input, nondegenerate faces,
no improper self-intersection and compatible shell nesting. M2 topology alone proves none of the
last two properties. First implementation uses bounded candidate triangle-pair traversal and checked
predicates; exact or interval-refined dyadic orientation/side classification must resolve sign.
A distance oracle returns certified inside/outside or indeterminate plus a distance-error interval.
Ray-edge/vertex contact uses independently specified ownership or a different deterministic ray;
never an unexplained epsilon. Work exhaustion/ambiguous sign is a typed failure. A plain f64 nearest
normal, chamfer transform or unbounded randomized ray is not a substitute for this admission.

The initial uniform-cell design uses distance Lipschitz bounds: for cell side h and center distance
interval [a,b], all cell values lie in [a-r,b+r], r=sqrt(3)*h/2 plus explicit coordinate-rounding bound.
Classify only when intervals prove material/void membership. Boundary cells require explicit bounded
refinement/reconstruction policy; never silently label an unresolved sign or topology as solid.
A selected conservative boundary approximation must report its displacement envelope, not exact
thickness. Emit shared canonical lattice vertices and consistently oriented boundary faces; reject
ambiguous edge/corner-only connections until an independently validated ownership strategy exists.
Cavity and hole connectivity uses explicit void-cell adjacency with interval-certified openings;
indeterminate throats stay indeterminate. Unknown connectivity never becomes drained/connected.

Provisional B engineering envelope: finite placed coordinates within +/-10000 mm; cell size
0.0001..10 mm; <=16777216 cells; <=2000000 emitted triangles; <=512 MiB requested geometry payload.
These are admission limits for forthcoming implementation, not measured performance or success claims.
Preflight bounding-box cell counts with checked integer arithmetic before allocation. Account together
for classification, connectivity queues, edge/vertex maps, triangles and source-query scratch. Use
streamed slabs where they preserve connectivity metadata. Terminate when operation counts exceed
explicit limits; retain no partial successful mesh. B must measure time/RSS against declared fixtures
before adopting a latency promise. The plan substrate permits wider values than a future backend;
backend admission must reject the unsupported subset explicitly.

Error accounting separates source tessellation, placed-coordinate arithmetic, distance-query error,
cell reconstruction error and downstream M3/M4 sampling. Compose supported length bounds by addition
with named units; volume needs independent absolute/relative tests, not conversion from a guessed
length epsilon. No finite Hausdorff bound alone proves topology/connectivity. B cannot be accepted
until its actual reconstruction supplies a justified bound or explicit indeterminate outcome for
each required fixture. Refine within cap or fail if the requested thickness/clearance cannot be
resolved; never clamp thickness or grid resolution silently.

### C/D geometric contracts beyond parameter admission

Use face-sharing (six-neighbor) void-cell adjacency with a certified positive-area opening. Edge or
corner touch alone is not an established drainage connection. Require a path from the intended cavity
to the exterior; preserve per-cavity enclosed/connected/indeterminate outcomes. Material components
also require certified positive-area/volume joining; a sampling resolution alone cannot certify a
point-contact support. Clearance uses lower/upper distance intervals against an explicit required
clearance, returning insufficient/established/indeterminate rather than a hidden tolerance.

The initial D support design is a union of a vertical platform base cylinder, a straight shaft,
a frustum and spherical contact tip. Let B=(baseX,baseY,baseHeight/2), C be contact and
u=(C-B)/|C-B|. Tip sphere center is C-u*(tipRadius-penetration); D requires penetration<=tipRadius.
The frustum ends at the sphere's equator (radius tipRadius), starts 2*shaftRadius earlier along u
(radius shaftRadius), and the shaft joins B to that start. This fixed taper length is an explicit
initial shape rule, not a commercial preset. D must reject nonpositive shaft length, platform/base
interference and unsupported inclination/contact cases, and independently verify intended material
intersection versus other collisions. Parameter-valid A requests can fail these geometric checks.
The base extends worldZ0..baseHeight and overlaps the shaft at B. No automatic branch placement,
raft generation, mechanical strength model or inferred source normal is included.

## Independent fixtures and acceptance

- Cube side20mm, t2mm: cavity side16mm, cavity4096mm3, shell3904mm3 under Euclidean inward-distance
  semantics. Refined grid shells must converge within the actual B bound; validate volume separately.
- Sphere radius10mm, t2mm: inner radius8mm; shell volume `(4*pi/3)*(1000-512)` mm3, separating input
  tessellation error from evaluation error. Reject t>=radius or explicitly report cavity collapse.
- Cube top-wall cut radius1mm directed from Z21 to Z17 reaches cavity through outerZ20/innerZ18;
  shallow cut endingZ19 misses the cavity. Missed, overlapping and near-throat cuts need exact or
  interval-independent occupancy/connectivity oracles and an indeterminate result when unresolved.
- Elevated box plus vertical support tests intended overlap, shaft/base contact, platformZ0 and
  unintended collision separately; material union must retain both components without parity holes.
- Open/duplicate/nonmanifold/self-intersecting, nested-shell, zero-scale and thin-feature fixtures
  exercise explicit unsupported/invalid/indeterminate outcomes. Add cancellation and exact/one-over
  cell/triangle/memory/record limits; no source mutation or stale result publication is permitted.
- A tests cover canonical records, full-prefix anchors, order and scalar failures, deterministic
  replay identity, bounded undo/redo/branching, ABA generation and failed/cancelled atomic updates.
  B–E add actual geometry/oracle/native-platform/M3–M5 export tests; A tests do not substitute for them.

M5.5-A provides the contracts and executable plan/history substrate. B–E remain unimplemented.
Native acceptance is reported separately from Linux; no physical drainage or support-strength claim
follows from any of these geometric specifications.

## Primary mathematical background

Hart's [Sphere tracing paper](https://graphics.stanford.edu/courses/cs348b-20-spring-content/uploads/hart.pdf)
provides the Lipschitz-bound background; the cell-center interval above is our direct application of
the distance function's unit Lipschitz property to the cube's half diagonal. It is not a claim that
sphere tracing implements mesh hollowing or certifies topology. Lewiner et al.,
[Efficient implementation of Marching Cubes cases with topological guarantees](https://www.ks.uiuc.edu/Research/vmd/projects/ece498/surf/lewiner.pdf),
discuss face/interior ambiguity for trilinear interpolants. That supports retaining reconstruction
ambiguity as a separate obligation; it does not prove preservation of the original mesh topology.
No tables or implementation from these papers are imported.
