# Bounded multiple-support stress fixture

`cargo run --release -p karasu-core --example preparation_stress -- NEW_DIRECTORY`
creates `report.json` and two binary PGM masks. The destination must not exist.
The source is the adjacent owned `source.stl`, using its recorded original-byte
digest. This is a reproducible bounded workload, not a printer calibration.

The 20 mm cube is translated to Z10, hollowed by 2 mm and drained by a finite
2 mm diameter bottom opening centered at XY(5,5). Eight support centers are the
Cartesian product X={9,12,15,18}, Y={3,9}. Each recipe has rectangular sections
at Z={0,1,9,10.3} with full widths {2,2,1,0.4} mm. The support-to-support policy
requires certified separation. The completed scene has eight contact certificates
and 24 individually closed convex components; component meshes are not a final
Boolean union boundary.

The 200×200 panel covers XY[0,20] mm with single center samples and binary levels.
At Z0.5, eight independent 2×2 mm rectangles expose exactly 3,200 pixels.
At Z11, the bottom shell minus the radius-1 drainage passage exposes 39,684 pixels.
The integration test independently checks every base pixel against rectangles,
and every Z11 pixel against an analytic circular opening except within the
explicit 0.001 mm radial approximation band. It also checks all 24 components
for a single closed combinatorial surface. Exact plan admission boundaries are
eight/seven recipes, four/three stations and four/three profile vertices.

The example's `elapsed_geometry_ms` includes fixture decoding, plan admission
and geometry generation. It excludes layer construction, rasterization and file
output. Timing changes are expected; canonical plan bytes, counts and masks are
deterministic. No performance acceptance budget or isolated-host measurement is
implied by a smoke run. Declare budgets before a measured campaign and retain
host/toolchain identities, complete results, original failures and peak-RSS scope.
