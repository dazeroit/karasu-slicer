# M3.5-B Uniform Layer Schedule Contract

## Scope and authority

The owner requested reference research, autonomous independent specification and M3.5-B
implementation on 2026-10-17. This authorizes the explicit schedule gate P-M3-027, not M3.5-C,
M3.6, a product-compatible default, rasterization or print-job metadata. TD-027 records the decision.
Reference observations do not select our numerical algorithms. EXP-03 and M1 remain open.

## Public values and terminal rules

`slicing::schedule` provides a validated `UniformScheduleSpec`, `ScheduleEnd::{Count(u64),
Before(f64), Through(f64)}`, explicit `ScheduleLimits`, and an owned immutable `LayerSchedule`.
The spec requires finite first world-Z in mm and finite strictly positive nominal height in mm.
Terminal Z must be finite. Validation order is first Z, height, then terminal Z; signed zero in
first/terminal Z becomes positive zero. There are no implicit defaults or geometry-dependent bounds.

For exact represented binary64 inputs a and h, mathematical plane i is a+i*h. Indices and counts
are u64 on every host; the largest accepted count is u64::MAX, with last index u64::MAX-1.
Count emits exactly the requested number, including zero. Before emits planes strictly below its
terminal; Through includes exact equality. Terminal comparisons precede rounding. A terminal rule
requiring more than u64::MAX layers fails with `IndexRange`; it never truncates. The exact probe at
index u64::MAX determines this condition without overflowing an index or host allocation.

Every `LayerPlane` contains contiguous index, finite published world-Z and the unchanged nominal
height. Publish each exact dyadic value rounded once to binary64, round-to-nearest ties-to-even.
Normalize exact zero to +0. Reject infinity-producing rounding with the offending index. Published
Z values must increase strictly; any duplicate rounding fails with both index and scalar context.
An exact-before-terminal plane can round onto the terminal; that does not alter membership.

## Independent arithmetic and capacity proof

Let eta=2^-1074 mm. Every finite binary64 is an integer multiple of eta, with absolute integer
magnitude less than 2^2098. Multiplying positive h by any u64 index yields less than 2^2162;
adding signed a has absolute magnitude less than 2^2163. Thus 34 u64 limbs (2176 bits), plus sign,
cover every count probe, direct expression and intermediate exact recurrence, without heap storage.
The height and inputs are decoded by bit fields, not floating multiplication. A bounded lower-bound
search (at most 64 iterations) finds the first excluded index for a terminal rule. Each probe evaluates
the independent expression a+i*h exactly. This avoids linear work merely to reject a count cap.

After count/capacity preflight, generation starts at exact a and adds exact positive h between
planes. This integer recurrence is mathematically identical to a+i*h and has no rounding drift.
Rounding retains the highest 53 bits, guard and sticky information, increments on greater-than-half
or odd-significand ties, renormalizes carry and checks the finite binary64 exponent. Subnormal
results are exact integer eta multiples. Signed rounding is symmetric; signed cancellation gives +0.
Checked limb overflow is a defensive typed error, not a hidden saturation or panic.

Repeated f64 addition was rejected because it accumulates error. Ordinary multiplication/addition
can round twice; f64 conversion of a u64 index loses integers beyond 2^53. FMA alone does not fix
that index loss. General arbitrary-precision dependencies and rational division are unnecessary:
fixed storage and exact monotone search cover this bounded domain. No fast floating filter or
parallelization is introduced without measured need.

## Resources, ownership, failures and cancellation

Limits are explicit: `max_layers: u64` and `max_payload_bytes: usize`. Limits are policy, not defaults
for physical slicing or guarantees about elapsed time/RSS. Count derivation precedes count cap;
count cap precedes host usize conversion, checked payload/isize capacity and byte cap. These precede
one fallible exact reservation, then per-plane range/resolution checks in index order. A zero count
allocates nothing. Payload is count*size_of(LayerPlane); it excludes allocator overhead, possible
over-allocation, fixed stack scratch and retained caller data. Host storage failure does not redefine
the u64 mathematical domain. No successful partial schedule escapes any error.

Ordinary and cancellation-aware builders share the same implementation. TD-014 checkpoints are
ScheduleStart (before any work), ScheduleCount (before every bounded-search probe), ScheduleStorage
(both sides of nonempty reservation), SchedulePlanes (at indices 0,256,...), and ScheduleComplete
(before publishing, including empty output). An observed cancellation preserves its checkpoint/cause.
An ordinary failure before the next check keeps precedence. Fixed arithmetic, allocation and cleanup
are indivisible; this is no wall-clock latency promise. There is no progress/job controller in M3.5-B.

Cost is O(34*(64+N)) time and O(N) requested output plus fixed scratch. Count-only schedules avoid
the search. Exact integer recurrence avoids repeated multiplication per output without changing
semantics. Immutable slices expose planes; this is a validated plan, not a slicing iterator or mesh
processing engine. M3.6 will own layer execution, progress and orchestration.

## Validation boundary

Tests must cover input precedence, empty counts, all terminal rules, exact versus rounded endpoint
membership, nearest-even ties of both signs, exponent carries, subnormals, maximum finite overflow,
resolution failure, caps and every cancellation checkpoint. Private tests cover fixed-capacity
extremes and indices above 2^53 and at u64::MAX. Independent integer/rational oracles and deterministic
raw-f64 sweeps verify exact expression rounding and terminal membership without calling production
arithmetic. Bit-level golden cases are portable; native Windows validation remains separately
reported. No M3 completion, reference compatibility or sustained fuzz/performance acceptance follows
from this increment alone.
