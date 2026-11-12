# Parametric support artifact validation

Generate the owned fixture into a new directory:

```console
cargo run --release -p karasu-core --example support_reference -- /tmp/karasu-support-fixture
python3 -m unittest discover -s tools/m55d-validate -v
python3 tools/m55d-validate/validate_support.py /tmp/karasu-support-fixture
```

The example emits three independent convex support-component meshes, authored
stations, a synthetic source cube, five PGM masks and an advisory contact report.
It also streams five layers through the prepared M3 engine, M4 rasterizer and M5
CTB/GOO/Photon/PP1 exporters plus reference PNGs. The synthetic 20 mm panel and
4.5 mm layer spacing deliberately traverse different support/model sections.
These parameters are a format fixture, not a calibrated or printable preset.
The source stamp is a caller assertion for synthetic geometry, not an asset hash.

The reader uses Python `Fraction` for represented mesh coordinates, exact signed
volume, oriented edge incidence and exhaustive vertex-to-face convexity. It
checks one connected boundary component, positive volume and the exact authored
station corners of each generated loft. This is independently authored validation;
it imports no production Rust helper. It does not establish general vertex-link
manifoldness, exhaustive coplanar-contact absence, arbitrary support contact,
reverse erosion inclusion or physical strength.

All 200,000 PGM samples are compared with an analytic square/taper/cube/cavity
oracle at exact rational pixel centers. Exact boundary coincidences use the
published half-open convention. There is no indeterminate band or mismatched
pixel allowance. Five reference PNGs add 200,000 analytic samples, including
support base, shaft, tip and hollow model. Existing independent M5 readers decode
all four machine formats and compare their masks with those validated PNGs.
They also check format-specific metadata and bounds. This proves owned-fixture
format integration, not firmware acceptance or compatibility with an actual
printer. `--without-machine` checks geometry/PGMs only and explicitly reports zero
reference PNG samples and no machine formats.

The sole variable report field is `geometry_elapsed_ms`, covering support
construction and validation after immutable plan creation. File generation,
rasterization, exporters and independent readers are outside that timing.
Compare geometry, mask and machine artifact bytes across native platforms; compare
reports after excluding this timing field. Run readers outside any performance
measurement. Preserve original rejected geometry and native launch failures as
separate evidence rather than overwriting them with successful retries.
