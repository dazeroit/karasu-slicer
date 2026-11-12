# M3.4-B Contract: Cycle Area and Traversal Orientation

**Status:** Owner approved on 2026-10-16 after the research/design handoff. Implemented at the
nonconstructive measurement scope under TD-024; actual Linux/native results are recorded in the
test strategy. Approval does not authorize dependencies, construction/regions or a commit.

**Baseline:** `main`, `2ca7e7e9718c3e3ebd5fb9fe65f1769f4e49b4e2` (implemented M3.4-A).
**Trace:** ROADMAP M3; GEO-001, SLICE-002/003, APP-001; TD-005/014/017/022/023.

## 1. Smallest increment and approval boundary

Measure the area magnitude and exact traversal orientation of **locally simple represented-XY
cycles already identified by M3.4-A**. Return a complete immutable per-path report borrowing that
inspection. Do not rerun contact detection, create or modify geometry, or accept filled material.

TD-023 approved detection only and explicitly left area/sign decisions separate. Approved TD-024
extends its staging exception solely to this nonconstructive measurement pass. It does not
settle, waive or postpone again the physical integer representation/scale decision required before
conversion, polygon construction or regions. No further staging exception is implied.

Excluded: algebraic-area reporting for invalid/open paths, rewinding or orientation normalization,
outer/hole classification, containment, nesting, union/material-area totals, Boolean operations,
intersection construction, repair, closure, snapping, physical grids, layer scheduling, rasterization,
printer/profile coupling, CLI commands, dependencies, threads and progress infrastructure.

EXP-03 remains a gate for final layer placement, not measurement of an already supplied section.
EXP-04 remains compatibility research, not a source of numerical constants. Neither experiment,
M1, whole M3 or later preparation/GUI milestones is completed by this increment.

## 2. Input, eligibility and ownership

Input is one immutable, privately constructed `PlanarInspection`, plus explicit `AreaLimits` and
optionally the existing synchronous `Cancellation`. Borrow its `SectionPaths`, graph, normalization
and raw provenance transitively. Read agreed finite world XY coordinates in millimeters, without
rescanning source faces, transforming/recomputing intersections or choosing conflicting points.

Return exactly one outcome per original path, in unchanged path-ID order:

| Upstream condition | Outcome |
| --- | --- |
| `SimpleCycle`, consistent clean closed cycle | Measured orientation and magnitude outcome |
| Open chain | Skipped: OpenPath |
| Ambiguous trail, even if graph-closed or coordinate-closed | Skipped: AmbiguousPath |
| `NonSimpleCycle` | Skipped: NonSimpleCycle |
| Inconsistent privately constructed status/path/coordinates | Typed operational consistency error |

Skipped paths have no fabricated area or orientation. A nonzero algebraic shoelace sum is never
used to promote a bowtie or ambiguous path. No selector, repair mode or tolerance is necessary.

Consistency checks precede the table's skip/measurement classification: embedding and path counts
agree; each referenced path slice exists; SimpleCycle implies ClosedCycleCandidate, graph-closed,
empty path issues, at least three vertices, equal edge/vertex counts, and zero collapsed/self-contact
counts. Ineligible corresponds to open/ambiguous kinds; NonSimpleCycle remains a closed candidate
with a reported collapse or forbidden self-contact. Validate measured coordinate references and
finite agreement without re-running the upstream contact certificate. A contradictory privately
constructed inspection is an error, not a successful skip that hides the contradiction.

Every cross-path diagnostic remains accessible through the borrowed inspection. Touching, crossing,
overlapping, coincident and nested paths can each have locally simple cycles and local measurements;
those observations do not establish compatible material or hole roles. Inherited complete-only
internal/unresolved groups and isolated vertices remain explicitly unassessed. Do not copy or hide
them, certify their absence, or expose `valid_material`/`printable`/`total_material_area` shortcuts.

Signed zero compares numerically as zero; original coordinates, bits and feature identities remain
unchanged. Exactness is about **represented binary64 endpoints**, not ideal source geometry before
STL encoding, transformations or M3.1 construction. This is not a new upstream error budget.

## 3. Geometric and mathematical meaning

For the stored ordered cycle `(x_i,y_i)`, let the successor of the final stored vertex be the first:

```text
D = sum_i (x_i*y_(i+1) - y_i*x_(i+1))
signed_area = D/2 mm^2
```

The wrap edge is counted once. Stored paths omit a repeated terminal vertex; evaluating an already
closed cycle's wrap is not automatic closure of an open path. No vertex is appended or reordered.

Exact positive D means Counterclockwise when viewed from +Z toward the XY plane; negative D means
Clockwise. This measures the current canonical path traversal, which TD-022 orders by graph IDs,
not source face winding or material role. The architectural outer-CCW/hole-CW convention applies
only after later validity/containment/role decisions; it does not instruct this pass to reverse rings.

A genuinely simple closed polygon with noncollapsed straight edges has positive enclosed area:
its bounded interior is nonempty/open, and Green's area integral gives shoelace area with one sign.
Therefore an exact zero sum for a claimed `SimpleCycle` is an invariant contradiction. Return
`InvalidInspection` with path context; do not return a successful zero-area ring or a Zero orientation.
Private arithmetic tests still cover exact-zero cancellation independently of public eligibility.

## 4. Independently justified exact arithmetic

All formulas below use numeric coordinates expressed in mm. Let eta=2^-1074. Each finite binary64
coordinate is exactly X*eta for an integer X. For encoded exponent e and fraction f, its magnitude
is f if e=0, otherwise `(2^52+f)*2^(e-1)`. Both zero signs have integer magnitude zero.

The maximum coordinate integer magnitude is:

```text
B = (2^53-1)*2^2045 < 2^2098
S = sum_i (X_i*Y_(i+1) - Y_i*X_(i+1))
signed_area = S*2^-2149 mm^2
M = abs(S)
```

Sign(S) is exact orientation. Each product is bounded by B^2<2^4196. Each complete cross term is
bounded by 2B^2<2^4197; after k full terms and the next first product, the bound is `(2k+1)B^2`.
Thus **every partial state**, even when adding the two signed products separately, obeys
`abs(partial) <= 2*N*B^2 < N*2^4197` for N closing edges. A sufficient magnitude bit count is
`4197+ceil(log2(N))`, with a separate sign, for N>=1.

Fixed scratch: **134 u32 magnitude limbs (4288 bits) plus sign**, for a checked per-cycle
N<=u64::MAX. At that domain limit the sufficient bound is 4261 bits. Check the count domain and
usize/isize arithmetic without overflowing while evaluating a guard; do not calculate `2*N` in usize.
The old 132-limb three-point predicate is not silently reused as an arbitrary-length accumulator.
Its 4224 bits cover this loose area bound only through N<=2^27, not arbitrary raised caller limits.

The final simple-cycle area has a stronger bounding-box bound, but that does not bound all partial
shoelace sums. Cancellation in the final answer does not justify smaller intermediate storage.

Use a narrowly private area accumulator, preserving the existing orientation arithmetic and
tests. Decode sign, <=53-bit mantissa and shift [0,2045]; multiply mantissas exactly in native u128
(<2^106), then add/subtract the shifted product into fixed limbs. The shift is at most 4090 bits.
Use u32 slices/u64 shift fragments: an aligned product spans at most five limbs, and a 106-bit
coefficient shifted left by 31 **does not fit u128**. Define/prove carry, borrow, magnitude comparison
and canonical zero sign; no per-edge heap allocation or public big-integer abstraction.

These integers exactly encode existing represented values. They are not a physical polygon grid,
snapping step or epsilon; no coordinate or topology is changed. Any small private helper reuse
requires review and must not broaden the existing orientation proof or alter its behavior.

### Alternatives and decision rationale

| Candidate | Assessment for this contract |
| --- | --- |
| Naive f64 shoelace/sign | Reject: product rounding, cancellation, overflow/underflow can lose the exact sign |
| Compensated summation | Reduces addition error, but cannot recover already rounded products or by itself cover all finite exponents |
| Rigorous floating filter with exact fallback | Possible later optimization; sign certification alone does not prove correctly rounded magnitude; needs independent interval/range proof |
| Adaptive expansions | Possible with a proved exponent domain/fallback and bounded work/storage; do not assume every error-free transform handles extremes |
| Fixed exact dyadic sum, one final rounding | Recommended correctness reference: elementary full-domain bound, deterministic and dependency-free; fixed limb cost needs later measurement |
| Arbitrary precision dependency | Unnecessary for this bounded numeric domain without separately justified need/review |
| Physical integer quantization | Changes represented geometry and remains outside scope/gated |

This explicitly resolves the earlier design's requested comparison with a checked compensated
estimate plus rigorous uncertainty bound: such an estimator must bound product as well as summation
error, protect the full exponent range, and fall back on uncertified sign/rounding. Ordinary
compensation alone cannot do this. A rigorous filter remains a valid alternative, but certifying
correct rounding near ties still requires exact information in the worst case. Approved TD-024
selects the full-domain exact reference instead of adding an unmeasured estimator and fallback;
approval settles this first-API choice, not the performance merits of a future proven filter.

For delta=2^-52, triangle `(0,0),(1,1+delta),(1-delta,1)` has exact twice-area delta^2,
while ordinary f64 products can both round to 1. Compensation of their subsequent sum cannot
restore the missing product information. Reference-product constants do not solve this problem.

## 5. Area magnitude and conversion contract

Do not convert S/M to f64 before scaling: that can overflow early or double-round. Specify one
positive binary64 conversion of exact `A=M*2^-2149`, **round to nearest, ties to even**, derived
from integer bits independently of ambient floating-point rounding mode.

| `AreaMagnitude` | Meaning |
| --- | --- |
| `Finite(f64)` | Strictly positive, finite correctly rounded magnitude in mm^2, including positive subnormals; not necessarily an exact scalar |
| `RoundedToZero` | Exact A>0 rounds to zero; orientation is still exact and the ring is not called geometrically degenerate |
| `Overflow` | Correct nearest-even conversion would produce infinity; retain exact orientation without returning an infinity scalar |

These are complete per-cycle **representation outcomes**, not warning-only loss, allocation errors,
geometry failures or claims about CPU exception flags. Consumers must match the outcome; there is
no unqualified zero, NaN, infinity or negative zero scalar. No optional rounding mode is introduced.

For M>0, let b=bit_length(M), t=max(1075,b-53), with guarded/saturating subtraction. Retain
q=floor(M/2^t); use guard/sticky bits and q parity to round once. If rounding produces q=2^53,
normalize by q/=2,t+=1 without a second rounding. q=0 is RoundedToZero. For q<2^52, t=1075
and q is the subnormal fraction field. Otherwise the unbiased exponent is t-2149+52;
an exponent above 1023 is Overflow. Construct finite positive bits only after range classification.

Exact boundary obligations:

| Boundary | Integer M / expected decision |
| --- | --- |
| Nonzero rounds to zero | `0<M<=2^1074`, including the half-min-subnormal tie |
| Normal/subnormal midpoint | `M=2^1127-2^1074`; tie chooses minimum normal, not maximum subnormal |
| Finite/overflow midpoint | `M=2^3173-2^3119`; equality and larger values overflow |

The overflow midpoint is `2^1024-2^970` mm^2 = MAX+2^970. An exact area greater than MAX
but below this midpoint can still round to finite MAX. Do not classify merely A>MAX as Overflow.

For a finite result, the only measurement-stage error is this final scalar rounding. In the original
rounding quantum `2^(t-2149)`, absolute error is at most `2^(t-2150)` mm^2 (use t before a carry).
For subnormal rounding and RoundedToZero the absolute bound is 2^-1075 mm^2. A relative bound
<=2^-53 applies when the exact A>=2^-1022 and conversion is finite; do not apply it indiscriminately
to subnormal A or infer a bound on ideal pre-rounding source geometry. Some bounds are smaller
than the smallest f64; do not store their rounded zero as a claimed exact numeric error bound.
No serialized error-bound field is required for this first API.

## 6. Public API and typed outcomes

Public entry points (lifetimes omitted here; see executable Rust documentation):

```text
slicing::area::measure_cycle_areas(&PlanarInspection, AreaLimits)
    -> Result<SectionAreas, AreaError>
slicing::area::measure_cycle_areas_with_cancellation(&PlanarInspection, AreaLimits, &mut Cancellation)
    -> Result<SectionAreas, AreaError>
```

`SectionAreas` borrows the inspection for the report lifetime, preserving its existing transitive
lifetimes. Accessors expose `inspection()`, `outcomes()` and `summary()`. `outcomes()[path_id]`
corresponds to that original path; do not sort by orientation, magnitude, role or allocation address.

`PathAreaOutcome` is `Skipped(AreaSkipReason)` or
`Measured { orientation: CycleOrientation, magnitude: AreaMagnitude }`.
`AreaSkipReason` is OpenPath, AmbiguousPath or NonSimpleCycle; `CycleOrientation` is Clockwise or
Counterclockwise. Eligibility/status mismatches are consistency errors, not additional skip reasons.
All summary counts are deterministic and conservative: paths equal measured plus the three skipped
counts; measured equals finite plus rounded-to-zero plus overflow counts. Report assessed terms,
input path vertex/edge reference counts and requested owned capacity. Do not sum per-cycle areas
into a material total, even when all rounded magnitudes happen to be finite.

`AreaError` must distinguish `Cancelled(Cancelled)` with its source, `LimitExceeded(resource,limit)`,
`SizeOverflow`, `Allocation`, and `InvalidInspection` with known path/vertex and a structured reason
(association/eligibility, cycle shape, finite coordinate agreement, or exact-zero SimpleCycle).
Do not panic for these recoverable/defensive failures or disguise scalar range outcomes as errors.

## 7. Determinism, limits, complexity and cancellation

Configurable defaults (safety policy, **not measured performance or job acceptance**):

| `AreaLimits` field | Default |
| --- | ---: |
| `max_paths` | 3,000,000 |
| `max_path_vertices` (all input paths, including skipped) | 6,000,000 |
| `max_path_edges` (all input paths) | 3,000,000 |
| `max_terms` (sum of actually assessed closing edges) | 3,000,000 |
| `max_payload_bytes` | 512 MiB |

Preflight counts all path records/references, including skipped input; checks all arithmetic/domain
and output count/byte caps before fallible reservation. Every cycle must obey N<=u64::MAX regardless
of configured limits. There is one flat outcome buffer sized to the original path count. Any added
heap scratch must be independently charged, not hidden. Requested bytes exclude borrowed upstream
allocations, allocator overhead/over-allocation, transient reallocations and fixed arithmetic stack;
they are not process RSS. Document actual type/capacity accounting during implementation.

Use O(P+134*T) work for P paths and T assessed closing edges, O(P) requested output and bounded
fixed scratch. Skipped paths require only bounded metadata inspection, not coordinate scans. Borrow
graph/diagnostics without expanding occurrences or iterating unrelated complete-only geometry. Do
not repeat M3.4-A's O(E^2) scan, allocate O(E) coordinate copies, or introduce a parallel f64 reduction.

TD-014 checkpoints: AreaStart, AreaInput, AreaStorage, AreaTerms, AreaRounding,
AreaRecords and AreaComplete. Entry polling precedes caps; metadata/inner accumulation/outcome loops
poll every 256 from zero. Count inner terms globally across cycles so one long cycle cannot hide
work between path polls. Poll before/after nonempty reservations and fixed per-cycle conversion;
final publication polls even for empty/all-skipped input. Fixed product/limb updates, conversion,
allocation and cleanup remain indivisible; there is no strict time-to-cancel or progress guarantee.
Ordinary failures before the next checkpoint retain precedence. Cancellation or operational error
returns no successful partial report; magnitude range/skipped outcomes are complete data, not partial
publication. Retry must see identical source/inspection/provenance and output.

## 8. Deterministic acceptance tests

| Family | Required independent assertion |
| --- | --- |
| Triangle, rectangle, convex/concave and forward-collinear rings | Exact rational sign and correctly rounded area in actual stored traversal |
| Open/ambiguous/self-crossing/backtracking/collapsed context | Explicit skipped outcomes; no algebraic area or invented closure |
| Disjoint/touching/crossing/overlapping/coincident/nested cycles | Local measurements remain separate; upstream contacts/uncertainty and IDs unchanged; no hole/material roles |
| Small integer/dyadic oracle | Expected area from rational determinants/integrals, not the production accumulator |
| Limb arithmetic | Carry/borrow/sign transitions, zero, cancellation, 53x53-bit products at every alignment residue mod 32, count-domain and maximum intermediate bounds |
| Severe cancellation and large translation | Naive floating failure examples do not change exact sign; original represented geometry is the oracle |
| Scalar conversion | Every normal binade, even/odd ties and carry, min-subnormal/normal midpoint, MAX/overflow midpoint and adjacent exact integers; independent rounding oracle |
| Range through real public input | Coplanar axis triangles of side 2^-600 and 2^600 mm reach SimpleCycle; area outcomes are RoundedToZero and Overflow with retained exact orientation |
| Permutation/metamorphic | Cyclic arithmetic rotation preserves area, reversal/reflection changes sign, lossless translation/scaling follows exact area law; canonical path direction remains ID-based |
| Resources/errors | Exact and one-below count/byte boundaries; checked overflow/domain; structured inconsistent-input/zero-area invariant errors; no truncated success |
| Cancellation/publication | Pre/input/reservation/inner-term/rounding/record/final and empty/all-skipped cases; precedence, deterministic retry and immutable inputs |

Reversing source face winding does not necessarily reverse TD-022's canonical path. Compare the
actual stored walk; use private arithmetic reversal tests or explicitly remapped source identities.
General floating translation/scaling can round or collapse points: metamorphic laws require every
represented transformation to be lossless and finite. Even power-of-two scaling can underflow.

Private rounder tests may use arbitrary in-bound accumulator states; do not claim every such state
is publicly reachable. Public range fixtures must use real upstream APIs, never fabricated
eligibility or weakened M3.1/normalization guards. Preserve all canonical STL/generator bytes.
Actual allocator exhaustion need not be artificially forced; report what was and was not exercised.

Implementation acceptance requires full stable format/strict Clippy/test/build, warnings-denied
private/public rustdoc, focused Debug/Release and separate native Windows validation are required.
Add a working API example and preserve all old exact-predicate/detector regressions. Record actual
commands/exits, source hashes and per-platform counts; do not reuse M3.4-A native results as new-code
validation. Any new fuzz/performance campaign requires explicit scoped reporting; no speedup is
claimed here and remaining M3 fuzz/resource/performance acceptance is not waived.

Implementation footprint: `crates/karasu-core/src/slicing/area.rs` with private arithmetic,
`crates/karasu-core/src/slicing/area/tests.rs`, a working rustdoc example, its declaration in
`crates/karasu-core/src/slicing.rs`, checkpoint variants in `crates/karasu-core/src/cancellation.rs`,
new `crates/karasu-core/tests/section_area.rs`, and the relevant architecture/test/decision/roadmap
records. Keep the triangle-plane contract, accepted upstream behavior, exact orientation arithmetic,
CLI, Cargo files, fixture generator and canonical STL bytes unchanged. `Checkpoint` is already
non_exhaustive; preserve that compatibility policy and existing operations' observed checkpoint
cadence rather than silently changing cancellation behavior. No dependency or source migration
is required.

## 9. Historical research checks — before implementation

The external exact model passed: 1,008 coordinate decoding checks, eight width cases, 504 ordered
triangle/reversal/cyclic checks, **19,925 binary64 rounding comparisons** against independent
Fraction-to-float conversion, **69,633 exhaustive toy-format nearest-neighbor comparisons**, and
eight lossless transformations. Additional assertions cover arithmetic cancellation, tiny/huge area, ties,
subnormal/normal and overflow thresholds. These finite models support, not replace, the proofs.

An external Rust probe using **only existing M3.4-A public APIs** confirms that both axis triangles
(side 2^-600 / 2^600) reach one SimpleCycle with no contact findings. Their independently analytic
areas are 2^-1201 / 2^1199 mm^2. This checks input eligibility, not a nonexistent area implementation.
Both model and probe exited 0. At that checkpoint no production code/tests or fixtures were changed.
Current implementation validation is separate in the test strategy.

The independent notes, mathematical derivation, models, probe and logs stay outside the
repository.

## 10. Sources and next gate

Public conceptual sources, not adopted code or geometry dependencies:

- [CGAL polygon concepts](https://doc.cgal.org/latest/Polygon/group__PkgPolygon2Functions.html): separate simplicity, area, orientation and containment.
- [OpenStax Green's theorem](https://openstax.org/books/calculus-volume-3/pages/6-4-greens-theorem): simple-curve area integral.
- [Oracle binary64 format](https://docs.oracle.com/cd/E37069_01/html/E39019/z4000ac019269.html): finite normal/subnormal representation.
- [Goldberg on floating-point arithmetic](https://docs.oracle.com/cd/E19059-01/stud.10/819-0499/ncg_goldberg.html): rounding/cancellation context.
- [Shewchuk's exact-arithmetic cautions](https://www.cs.cmu.edu/~quake/triangle.exact.html): range assumptions must be proved, not inferred from an algorithm name.

The limb bounds, conversion formula/thresholds and proposed Rust strategy are independently derived
here and checked by the external models, not commercial behavior or a copied library implementation.

**Approval boundary:** the owner accepted TD-024 and this single M3.4-B implementation after the
research handoff. Construction/regions and a physical grid require their own retained gate and
approval. New dependencies, whole-milestone closure and a commit are not authorized by this decision.
