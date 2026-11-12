# M4 LCD Layer Engine implementation contract

This contract is designed independently from reference behaviour and published facts.
Commercial mode parity is not asserted. No other program's code is copied.

## Coordinates and eligibility

The profile explicitly specifies physical XY minimum and extent in millimeters, pixel dimensions,
axis swap and independent output flips. Output is grayscale8, row-major; row zero is the first PNG
row. Without transforms world +X/+Y increase columns/rows. A swap occurs before flips. Continuous
panel boundaries are 0 and dimension; pixel centers are index+0.5. No printer defaults are selected.
Inverse conversion accepts finite continuous coordinates, including points outside the panel.

Strict complete M3 boundaries use world represented coordinates and even-odd filling. Unresolved
layers fail. Explicit constructed material uses its exact boundary edges, dyadic scale or the parent
layer's explicit grid origin/step. Rational coordinates are converted only at this lossy raster
boundary. Construction remains exact and immutable. Conversion or transform failure rejects the
whole image. Both strict and constructed routes preserve holes and disconnected regions.

Raster arithmetic uses finite binary64. Profile dimensions are 1..65536 on each axis. For input
coordinates and profile origins, abs(coordinate)/pitch + abs(origin)/pitch must be at most 2^24.
Constructed grid origins and scaled rational offsets must also satisfy this bound before addition,
preventing cancellation from concealing earlier rounding. The conservative projected-endpoint
displacement budget is 2^-20 pixels, including rational conversion and affine mapping.
A nearly horizontal edge can amplify endpoint Y error into a larger horizontal crossing shift,
so this is a geometric boundary-distance band, not a uniform exact-intersection-X bound.
Unsupported dynamic ranges and collapsed mapped nonzero edges fail explicitly.
Against the projected pixel-space edges, every sample is classified exactly (TD-050): an
interpolated crossing only estimates the first sample column at or right of an edge, and the exact
orientation sign of the sample centre against the edge fixes it. The result is independent of the
edge's direction and of interpolation rounding; only the projection band above remains
approximate. Horizontal edges do not contribute crossings. Subrows use lower-inclusive,
upper-exclusive Y membership; material spans include their left boundary and exclude their right,
so a sample centre exactly on an edge belongs to the material on the edge's right.
The permitted lossy raster conversion does not repair source geometry.

Outside-panel policy is explicit: reject any boundary vertex beyond the closed panel, or clip the
sampled material to the panel. Clipping preserves original crossings (no independently clamped
vertices); intersect spans with the image domain. Preview records this choice.

## Sampling and intensity

Binary is one center sample. AA is an explicit regular NxN midpoint grid for N in 1,2,4,8,16.
A row's coverage count is integer in 0..N*N. Quantization to L levels (2..256) first rounds
count*(L-1)/(N*N) to nearest with ties upward, then rounds level*255/(L-1) the same way.
No implicit gamma, blur, exposure floor or optical/resin correction is applied. Thus values mean
linear sampled geometric coverage; they are not measured emitted light. Sampling is not analytic
area integration: a feature smaller than a subcell may be missed. Pixels intersected by boundaries
form the coverage-error band; homogeneous subcells are classified exactly outside the arithmetic
band above. Before grayscale quantization, sampling area error is bounded by the area of subcells
intersecting the boundary, plus the arithmetic band. Quantization adds at most
1/(2*(L-1)) + 1/(2*255) normalized intensity error per partially covered pixel (zero at full/empty
endpoints); sum that term times pixel area for an integrated intensity/area comparison.
No universal boundary-length-free area-error claim is made.

After quantization optional discrete rectangular compensation uses explicit nonnegative X/Y pixel
radii and either grayscale maximum (dilate material, shrink holes) or minimum (erode material,
expand holes). The outside image is zero. Radius zero is identity. A rectangle in pixels is
anisotropic in millimeters when pitches differ, and is not a Euclidean contour offset. The UI/job
profile may later choose calibrated physical offsets; M4 does not invent such calibration.

## Algorithms and budgets

Sort edges by minimum Y once. Advance a sweep through subrows and maintain active edge IDs,
removing edges whose upper Y has passed. Sort the active crossings' exact sample columns per subrow;
pair adjacent crossings for even-odd material, or sweep their signed winding changes under the
winding rule (TD-050). Each row is written from the sorted span endpoints of its subrows as runs of
constant sample count, avoiding N*N point-in-polygon queries, an upscaled image and visits of
uncovered pixels. Explicit work charges
cover edge conversion, active scans, sorting charges (2*length*bit_width(max(length,1))),
span endpoints and morphology.
Sort charges are a deterministic budget model; they are not exact standard-library comparator counts.

Compensation is separable sliding extrema with a monotone index queue: linear pixel work independent
of radii. Temporary storage includes one extra full image and one line-index queue. Every reservation
is fallible, checked and preflighted against requested payload caps. Caps are not allocator/RSS
promises. Work/cancellation errors never publish a partial image. Sorting and allocation are
indivisible, with checks around them; work loops poll at most every 256 operations.

The image stores a validated profile, full configuration, layer index/Z, nonzero bounds/count,
intensity sum, and pixel data for headless preview. A fused iterator consumes M3 owned layers one at
a time; it retains no layer cache. Source failure or raster failure is terminal and preserves cause.
Caller retention of returned images and source iterator storage are outside its per-call budget.

PNG is deterministic grayscale8 with filter0 and stored DEFLATE, no timestamp/gamma/profile assets.
It writes bounded chunks, so no second encoded full image is required. Writer failures preserve
underlying I/O causes; writing to an arbitrary writer can leave partial bytes. See the PNG design.

## Validation gates

Analytic asymmetric masks, holes/islands, exact pixel boundaries, transforms and round trips,
independent point-sampling oracle, all AA levels/quantization endpoints, clipping, compensation
against a naive oracle, rejected geometry, resource caps, cancellation/fused streams, deterministic
PNG hashes and independent zlib/PNG decoding are required. Corpus replay validates M3-to-M4 routing.
Benchmark a realistic LCD and an edge-heavy layer against declared preliminary 5-second and
256-MiB requested-payload / 512-MiB process-RSS budgets. Run workspace fmt, strict Clippy, tests,
build, and native Windows checks separately. Any host launch block remains explicitly pending.
