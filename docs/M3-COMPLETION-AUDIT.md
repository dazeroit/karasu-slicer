# M3 Implementation and Acceptance Audit

## Current disposition

**M3 complete for the bounded declared corpus**, per the
[closure acceptance record](M3-CLOSURE-ACCEPTANCE.md). TD-032 fixes the additional corpus-discovered
shared-edge classification and makes the large fixture test self-contained. Corpus, preliminary
budgets, bounded fuzz/property/replay and native Release functional acceptance are complete.
Linux is **611/0/2**, native Windows Release **610/0/2**. Native Debug host Code Integrity restrictions
remain recorded environment failures; no failed command is relabelled as PASS. EXP-03/04 remain M1
compatibility research, not blockers for explicit M3 parameters. The chronological audit below
retains earlier limitations and pending states for provenance; it does not reopen accepted work.

## Baseline and authority

The audit starts from clean `7ee0440` on 2026-10-17. The owner requested that remaining M3 gaps be
identified, researched as reference behaviour, independently specified and implemented.

| M3 requirement | Code and result | Remaining acceptance or limitation |
| --- | --- | --- |
| Triangle-plane predicates, intersections and degeneracy | `geometry::section`, raw traversal | Represented-f64 geometry and explicit construction errors |
| Deterministic contours and diagnostics | `normalize`, `graph`, `paths`, `validity` | Open/ambiguous geometry is retained, not repaired |
| Area, grid and nested regions | `area`, `grid`, `regions` | ExactOnly domain and explicit strict material rule |
| Planes, layers, progress and cancellation | `schedule`, `engine` | Explicit configuration and complete owned atomic items |
| Required constructive material composition | NEW `construct`, `engine::composition` | Explicit source groups; parity within groups, union across groups |
| Crossings, contacts, coincidence and provenance | NEW rational arrangement atoms/cycles/forest | Individually simple source rings; contact vertices remain shared |
| Resource/partial-output policy | Stage and new work/capacity caps, typed errors | Requested-capacity/work limits, not hard RSS/time guarantees |
| Automated validation | Independent occupancy/provenance tests and dedicated constructor/strict-region fuzz targets | Short bounded campaigns do not prove defect absence |
| Performance/memory evidence | Release `m3_profile` example and external timing/RSS records | Preliminary large-input acceptance budgets still need explicit adoption |
| Cross-platform acceptance | Linux/WSL gates on changed code | Native Windows replay pending; current WSL interop registration absent |
| Commercial defaults | Retained M1 EXP-03/04 questions | No implicit default/precision/cleanup/runtime compatibility claim |

## Actual implementation gap

Before this increment, crossing/coincident individually valid cycles could only remain strict
`GeometricallyUnresolved` reports; material composition was unavailable. P-M3-028 was a real missing
implementation decision. The expanded-MVP model/support handoff also requires composition that does
not erase overlapping independent solids through accidental global parity.

TD-030 accepts the smallest required constructive operation: caller-named material sources,
even-odd within a source and union across sources, exact original-segment intersections and symbolic
side classification, all support provenance, and rational cycles/regions. See the
[contract](M3-CONSTRUCTIVE-MATERIAL-CONTRACT.md). The engine adapter requires one source ID per
original face and rejects a path with mixed ownership. Strict context is preserved; composition is
attached only before atomic publication. Existing strict callers retain their configuration/API.

## Domain and remaining work at the TD-030 checkpoint

Self-contacting source rings, open/ambiguous paths, unresolved ownership and off-grid input remain
structured failures. No source is silently repaired or reinterpreted. General offsets, arbitrary
Boolean command families, hollowing and support generation remain later scoped work. Spatial
acceleration/parallelism require measured benefit; their absence is not an empty mandatory module.
Lossy snapping remains unavailable without a positive physical/topology budget. The exact-grid
domain is explicit and does not promise successful slicing of arbitrary numerical meshes.
The profiling setup observed this restriction even at quarter-height planes of an analytic ribbon:
the unchanged approximate triangle-plane kernel can produce represented XY slightly off the exact
grid. The accepted benchmark planes are recorded explicitly, and no tolerance was inserted to make
rejected cases pass. Numerical-domain/corpus acceptance must review this limitation before claiming
general slicer usability; an explicit positive-error conversion would need its own proof/policy.

This closes the identified constructive gap for that domain. Whole-M3 acceptance still requires
revision-specific native Windows evidence, agreed-corpus review, preliminary large-input budget
adoption and remaining reference research gates. Baseline timings, Linux tests and small fuzz inputs
do not establish universal mesh validity, physical printability or cross-platform acceptance.


## TD-031 follow-up: mandatory ExactOnly bottleneck

A further implementation limitation was identified, rather than a reason to hide rejection behind
a tolerance. The [represented-coordinate contract](M3-REPRESENTED-COORDINATE-CONTRACT.md) supersede the mandatory-grid
restriction above. Strict layers can explicitly preserve agreed f64 millimeters and use exact
predicates without any integer conversion. Constructive source union accepts losslessly encoded
represented coordinates within +/-2^60 common units and retains physical exponent metadata.
Ordinary decimal rings and previously rejected quarter-height planes are covered end to end.
Legacy exact-grid callers keep their failures and contract; unsupported constructive dynamic range
still fails explicitly rather than snapping. Existing ambiguity/repair/source-ownership policies
remain deliberate domain boundaries.

No additional mandatory unimplemented M3 operation remains under the accepted conservative
contract. Source-tag stitching, half-open containment, schedule/owned publication, material
composition and diagnostics are implemented independently. Tolerance-based coplanar treatment,
manifold-fan assumptions, parallel scheduling and Z buckets are not adopted; changing those
policies or adding acceleration is not required to fill a missing M3 module.
Whole-M3 acceptance still needs current native Windows evidence, agreed corpus and adopted budgets,
plus remaining reference gates. This update does not mark those gates complete.


## Native Windows follow-up at 382fb62

The [native campaign](M3-NATIVE-WINDOWS-VALIDATION.md) now passes on 2026-10-17: **600/0/2**,
all required native quality gates, warnings-denied rustdoc, seven focused Release suites, and
150-file native SHA-256 integrity before/after. The earlier bridge diagnosis was incomplete:
authorized execution outside the sandbox reaches Windows despite the absent visible binfmt entry.
Direct PowerShell commands also avoid reliance on disabled script-file execution without changing
policy. No source fix is required and historical application-control error 4551 does not recur.
Native functional acceptance for commit `382fb62` is complete; agreed-corpus, preliminary-budget and
remaining reference gates still prevent whole-M3 closure. Earlier pending statements are historical.
