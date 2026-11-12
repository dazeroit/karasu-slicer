# M5.5-D parametric manual supports

## Requirements, research and domain

Use the established research → behavioral requirements → independent design → native-Rust
implementation → validation sequence. Reference research records the controls other programs
offer. Separate contact, tip, shaft, base, anisotropy, orientation and path parameters are observed; commercial
accuracy, material union, collision and strength guarantees are unknown. No proprietary algorithm,
foreign geometry kernel, sampled collision rule or reference implementation is imported.

The headless backend accepts one certified closed outward convex source, either solid or using the
existing M5.5-B inward cavity and optional M5.5-C finite cardinal cuts. Source placement is evaluated
first. The support itself may bend, incline, contain horizontal/returning segments and have independently
tilted/rolled sections. It is not restricted to vertical circular posts. Arbitrary concave STL source
preparation, automatic placement/island analysis, branching support trees, physical strength and GUI
editing remain outside this backend. A caller can express curved-looking paths as explicit stations;
no continuous Bezier sweep or unimplemented curve-accuracy promise is exposed.

## Full parametric recipe

Each `SupportRecipe` has a stable nonzero ID, exact upstream `SurfaceKey`, intended contact and at
least four ordered stations: platform footprint, base top, neck and penetrating endpoint. Additional
stations change shaft centers, sizes, shapes and orientations. Station order defines the path; later
centers must be above the platform but need not increase in Z. Vertex geometry below the platform
is rejected. Positions are world millimeters and rotations are existing right-handed `Rotation3`
quaternions, including arbitrary tilt and roll. Neighboring profiles need not share vertex counts
or shape type.

Supported explicit local profiles:

| Profile | Independent parameters |
| --- | --- |
| Elliptical polygon | X/Y semiaxes and side count; equal axes give a circular polygon |
| Rectangle | Full X/Y dimensions |
| Regular polygon | Circumradius and side count |
| Custom convex polygon | Ordered local XY vertices, permitting asymmetric shapes |
| Point | Sharp terminal endpoint only |

The polygonal profiles are the requested native shapes, not invisible approximations of smooth
curves. Increasing side count changes the explicit geometry. No certified circle/ellipse surface
error is inferred from nominal radii or the model's hollowing sampling setting. Trigonometric
candidates are independently checked as represented polygons. Custom polygons must be strictly
convex, counterclockwise and strictly contain their local origin; no sorting, recentering, welding
or repair is performed. Repeated, self-contacting, nonconvex or incorrectly wound inputs fail.
Every transformed non-Point ring must retain exact affine rank at least two after rounded placement;
this excludes rounded point/line connections even if neighboring hulls individually have volume.

## Independent geometry definition

Each adjacent pair of represented world rings defines its convex hull, built with the existing exact
3D predicates and independently validated hull topology/embedding. This is an explicit convex-loft
definition, not an implicit matching of polygon vertices or a hidden general Boolean repair. Section
rotation/twist changes the hull; it does not promise a continuously twisted analytic surface.
A terminal Point produces a cone/pyramid-like tip. All nonterminal shared rings have positive area,
so adjacent full-dimensional components connect through a complete section rather than only a point
or edge. Every component is closed and outward-oriented. The first ring must have positive area
exactly on platform Z=0, and all component vertices must remain at or above it.

`SupportedModel::components()` exposes these explicit component meshes for previews and component
export. It is deliberately not a final Boolean union-boundary mesh. The actual prepared solid is
`(model minus union drainage cutters) union support components`, evaluated by its layer engine.
Within a support, all components intentionally union. Between different recipes the caller chooses
`RejectUncertified` separation or explicit `Union`; the latter is intentional overlap, not a strength
or brace-connection certificate.

## Contact, penetration and collision certificate

A contact identifies a preserved placed-exterior source triangle, world position and maximum
penetration. The triangle selects its entire coplanar supporting facet, not a barycentric restriction
to one triangulation cell. No nearest-surface projection or implicit rebinding is performed.

The independently designed interval proof requires:

1. Contact position lies within explicit `contact_tolerance_mm` of the selected source plane and
   strictly within every tip-component support halfspace by more than that tolerance. Orthogonal
   projection to the true selected plane therefore remains inside the tip.
2. Every neck-ring point is strictly exterior to that plane; every terminal point is strictly
   inside it with at least `clearance_mm` penetration and at most the declared maximum penetration.
3. Every tip vertex lies strictly inside all other distinct exterior supporting halfspaces. Exactly
   the intended facet is entered; a corner crossing or other model penetration is not silently allowed.
4. Every non-tip component is separated from the model by a certified exterior support plane.
5. Every component is separated from the cavity and all drainage cutters. Bounding-box or supporting
   facet certificates are sufficient conditions; absence of a certificate is an uncertainty failure,
   not a mathematically established collision. Cross-support rejection similarly requires separation.

These conditions, full-dimensional convex tips and connected rings establish positive-volume model
contact and positive-area platform/path connection. A source containing no hollow operation is
validated as a solid, with no fabricated empty cavity. Cavity/cutter separation prevents supports
from entering the void or blocking the admitted drainage cut. Conservative proof failure reports
support ID, component and obstacle. Local contact validation is neither a complete collision solver
for arbitrary meshes nor a mechanical load/adhesion/printability certificate.

Distance normals and comparisons use outward intervals with the existing independently verified
square-root bounds. Exact predicates identify coplanar facets and represented profile rank. Published
minimum/maximum penetration bounds apply to represented geometry; clearance is a proven lower
admission margin, not an empirical material recommendation or global epsilon.

## Identity, editing and history

Historical `SupportSpec`, `PreparationPlan`, `KPREP1` bytes and existing history APIs remain unchanged.
The richer `SupportPlan` is separately versioned as `KSUP1`: full upstream canonical record, policy,
semantic contact/clearance tolerances, ordered support IDs, contacts, stations, quaternions, rounded
rotation matrices, profile tags and full dimensions/vertices. Counts/IDs are little-endian u64;
scalars are little-endian IEEE f64 with signed-zero canonicalization. The record is an identity
contract, not a new project-file loader. Source digest/decoded-geometry correspondence remains an
asset-loader assertion. Quaternion representation and explicit station/vertex ordering are retained.

A changed source/placement/hollow/cut prefix rejects old support keys. Support-only edits preserve
the upstream key. Moving the model never silently drags world-positioned supports or contacts;
rebind and edit explicitly. Resource caps do not change geometry identity; semantic tolerances do.
Evaluation with tolerances different from the admitted plan fails instead of reusing an old identity.

`SupportHistory` retains bounded immutable snapshots with add/edit/remove through new complete
plans, undo, redo, redo truncation after changed edits and monotonic owner-local generations.
Canonical-identical edits are no-ops. Cancellable variants check before content mutation; failed or
cancelled edits preserve current content, generation and redo. Consumers must bind async work to
both history owner and generation/canonical record; a counter from another history is not equivalent.
No disk persistence, GUI pick state or runtime job manager is introduced before its milestone.

## Exact layer and export composition

The new explicit prepared arrangement policy uses source 0 for body parity, IDs below bit63 for
cutter parity sources, and high-bit IDs for additive component parity. It computes
`(body AND NOT any cutter) OR any additive component` in a single exact rational arrangement.
Cutters never accidentally remove support material; intersection admission is handled separately.
Existing union/difference API behavior is preserved. Face labels are generated explicitly by the
scene, not inferred from source winding/connectivity. Strict source diagnostics stay available.
The compatibility `material_union()` accessor contains the selected constructed composition.

Exact arrangement semantics apply to represented f64 M3 section coordinates; original triangle
interpolation retains its existing contract. Rational arrangement intersections are not rounded back
to a physical grid. M4 consumes that constructed material and M5 streams masks through the existing
extensible exporters. Owned-fixture CTB/GOO/Photon/PP1/reference export is tested here; comprehensive
combined-scene and interactive acceptance remain M5.5-E and M6/M7.

## Limits, cancellation and reproducibility

Default limits: 32 recipes, 32 stations per recipe, 128 vertices per profile (hard cap256), 200 million
charged support work units and 512 MiB conservative cumulative capacity admission. Per-component
hull limits include 512 output vertices, 2048 output triangles, two million work units and four MiB
payload. Base hollow/drainage limits remain independent. Independent caps are ceilings, not a promise
that every combination at each maximum fits the aggregate policy.

Admission includes subordinate base work/storage allowances, an extra merged base copy and label/
bookkeeping envelope, local/world ring buffers, per-loft input vertices, component hull allowances,
retained/merged component capacities, metadata and canonical record. Profile work is precharged
quadratic edge/vertex comparisons plus linear placement/rank work; hull and base maxima are precharged.
Containment, obstacle bounds, separation pairs and assembly records charge the remaining work.
Checked arithmetic and fallible vector reservations reject overflow/exhaustion. Capacity allowances
are not total allocator traffic, realloc-copy costs or RSS. Canonical encoding grows geometrically
within its cap rather than reallocating for every scalar.

SupportStart, bounded SupportWork and SupportPublish guarantee atomic plan/scene publication.
Subordinate geometry retains its own cancellation checkpoints. Completed layer streams follow the
existing fused per-layer contract; job-level atomicity requires staged output. Allocation, sorting,
copy/destruction and fixed numeric primitives remain indivisible, not wall-clock guarantees.

Fixed input/order/build repeats deterministically. Host trigonometric proposals may differ between
platforms; certificates apply to emitted geometry. Native artifact equality is measured for owned
fixtures, not asserted universally for all arbitrary-angle/custom-profile inputs. Unsupported or
uncertain geometry never masquerades as validated output. See [validation](M55D-VALIDATION.md).
