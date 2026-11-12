# M5.5-E headless preparation integration specification

## Scope and independent design

This increment integrates the established A–D geometry contracts; it does not change their
supported domain or introduce a geometry dependency. The edit states other programs show inform
requirements, not numerical correctness.
The independent implementation remains stable, safe Rust in `karasu-core`.

The supported source is one closed, outward, embedded convex polyhedron. Hollowing uses the
certified inward halfspace backend; drainage uses admitted finite cardinal cutters; supports use
explicit polygonal stations and adjacent convex lofts under the D contact/separation policies.
Arbitrary concave sources, intersecting cutters, uncertified contacts and generic final Boolean
meshes are unsupported. Existing typed errors remain actionable; no automatic repair is added.
Coordinates are right-handed millimeters, using the established explicit local/world transforms.

## Revision ownership and state

`preparation::session::PreparationSession` owns an immutable `Arc<TriangleMesh>`, one asserted
original-asset SHA-256 identity, bounded `SupportHistory`, an opaque owner identity, a monotonic
request counter, lifecycle state and at most one retained completed scene. The asset loader must
establish the relation between original bytes and decoded mesh: the session does not verify it.
Changing the original asset digest requires a new session. An edit to placement or recipe stays
within the existing session and advances its history generation.

States are `Dirty`, `Pending`, `Ready`, `Failed` and `Cancelled`. Valid real apply/undo/redo edits
invalidate current access. Canonical no-op edits preserve readiness; rejected history edits preserve
prior state. `begin` issues an owned immutable `PreparationTask` and immediately makes the session
Pending, including a new request for the same plan. The caller chooses synchronous evaluation or
moves the task to its own worker. No scheduler, GUI framework or thread pool is introduced.

Evaluation consumes the task and returns an opaque `PreparationOutcome`. It holds either a whole
validated `SupportedModel` or its original structured failure. Acceptance checks owner identity,
history generation, newest request and complete canonical bytes. Foreign, stale, out-of-order and
edit→undo ABA results cannot replace current state, even when recipe bytes happen to match again.
Successful current publication alone establishes Ready. Current failure/cancellation preserves the
last completed scene for inspection, while denying current geometry. `recompute` follows the same
task/evaluation/acceptance boundary; it is not a shortcut around validation.

`current_scene` lends geometry only in Ready with a matching identity. Its Rust borrow excludes
concurrent mutable session edits while a caller uses that reference for layer generation/export.
`last_completed` exposes a labeled preview: original plan/canonical identity, components, cavity and
certificates, plus whether it is current. It deliberately provides no layer engine or raw supported
scene. Preview controls must use that retained plan, not the newer edited placement. Low-level
immutable snapshot APIs remain available for callers without editing workflows; this integration
gate is an application contract, not a security boundary.

## Progress, cancellation and bounded ownership

`generate_supports_with_progress` and task evaluation emit deterministic advisory `SupportProgress`
with Admission, Source, Profiles, Components, Validation, Assembly and Complete stages; support
counts and optional support/component identifiers are explicit. These are phase/count observations,
not a fabricated percentage or elapsed-time estimate. Complete means computation finished, not that
the session accepted the result. Superseded workers can finish and must still fail acceptance.
The final SupportPublish checkpoint precedes Complete. SessionStart and SessionPublish add
cooperative cancellation at ownership boundaries; existing nested geometry checkpoints are preserved.
Observers run synchronously and may request cancellation through the caller's shared token.

History byte/count limits and existing source/cut/support/geometry limits apply before publication.
A session retains at most one completed scene, but tasks/outcomes owned elsewhere can retain more
source/results. The caller must limit concurrent workers and queued outcomes. Capacity limits do
not promise a whole-process RSS or wall-clock ceiling. Canonical comparisons, owner checks and
value destruction include indivisible work; cancellation is cooperative, without a hard latency
promise. No partial scene replaces the last complete scene. Structured source/operation errors and
underlying cancellation causes are preserved through `SessionError`.

## Material and composed error contract

The M3 layer engine evaluates `(body − admitted cutters) ∪ additive support components`; hollow
body already subtracts its cavity. Exact represented-coordinate arrangement avoids overlap double
exposure and parity holes. M4 rasterization and existing M5 serializers consume the same layers.
The owned fixture explicitly tests closed exterior/platform caps and subtractive cavity caps.
No fused final mesh or physical printability certificate is implied.

The composed budget is a vector, not a sum falsely labeled wall thickness or Hausdorff distance:

- B certifies `K(t + δ) ⊂ cavity ⊂ K(t − δ)` in erosion-parameter space, preserving the exterior.
- C records radial polygon approximation and finite axial/cap admission separately.
- D represents the authored polygons exactly as the loft input, without promising an unstated
  smooth ellipse/cylinder target. Contact depth and clearance policies remain independently checked.
- M3 operates on represented intersections and exact rational planar material composition.
- M4 grid resolution, sample/AA choice and quantization have their own established contract.

A closed cavity is explicitly reported as enclosed; geometric admission does not establish venting,
resin evacuation, support strength, exposure calibration or hardware compatibility. Source concavity,
multiple cavities and uncertain geometry are rejected according to the component contracts.

## Validation and handoff

[Closure acceptance](M55-CLOSURE-ACCEPTANCE.md) records the executed scenario matrix, independent
owned STL/mask/file readers, cancellation, ASan, repeated Release measurements and full native
Windows/Linux checks. The model is imported through M2, hollowed, drained, supported, sliced,
rasterized and exported as CTB, GOO, Photon and Anycubic PP1. Source SHA correspondence is independently
verified for this owned fixture, not generalized to every caller.

M6 must provide actual editing controls, diagnostic/cavity/support preview, background orchestration,
current-only export and responsive cancellation on both platforms. If project persistence is added,
source verification and saved replay become mandatory; this increment adds no project format.
M7 retains distribution, interoperability and separately authorized physical trials. Completing this
bounded headless milestone does not complete the expanded MVP.
