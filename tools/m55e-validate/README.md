# Combined preparation validation fixture

Generate an absent output directory with the independent owned fixture:

```console
cargo run -p karasu-core --release --example preparation_reference -- /tmp/karasu-preparation-reference
python3 tools/m55e-validate/validate_preparation.py /tmp/karasu-preparation-reference
python3 -m unittest discover -s tools/m55e-validate -p 'test_*.py'
```

The fixture uses a 20 mm cube translated 10 mm above the platform, a 2 mm
hollowing offset, a bottom drain centered at `(5,5)` and rectangular support
stations centered at `(12,8)`. The example imports the owned ASCII `source.stl` in millimeters and binds its
known SHA-256 digest. The reader hashes the emitted bytes independently and
compares the imported geometry against the source vertices and facets. This
verifies this trusted fixture; the general preparation API still treats asset
digests as caller assertions.

The reader uses exact rational arithmetic for represented convex mesh
orientation, directed edge closure, connectedness and volume. It checks source
preservation, the reversed cavity append, station corner preservation and exact
analytic rectangular-loft volumes. These checks do not replace the Rust
contact, collision, embeddedness or hollowing inclusion certificates.

A separate analytic oracle checks 12 standalone binary PGM masks and all 28
reference PNGs. It includes platform, station, exterior and cavity caps, the
drain opening, tapered support and support/model union. Only pixels in the
reported radial-error annulus are permitted to remain indeterminate; counts
are reported separately. Subtracted cavity membership includes its bottom
cap, while the exterior and platform caps are material. These conventions are
explicit expectations, not learned from the emitted masks.

Existing independent M5 readers decode CTB, GOO, Photon and PP1, checking their
masks against the independently verified reference PNGs. This establishes
software-level format consistency, not successful printing, resin calibration
or printer firmware acceptance. The 0.5 mm synthetic layer spacing is selected
for boundary coverage, not recommended as a print preset.

`geometry_elapsed_ms` times scene generation only. It excludes fixture setup,
layer construction, rasterization and export. Whole-process measurements must
be recorded separately. Timings are intentionally variable; geometry and mask
bytes should remain deterministic for fixed input and supported platforms.

## Independent bounded stress reader

```console
cargo run -p karasu-core --release --example preparation_stress -- /tmp/karasu-preparation-stress
python3 tools/m55e-validate/validate_stress.py /tmp/karasu-preparation-stress
```

The standalone standard-library reader uses rational rectangle/circle membership to check 80,000
pixels independently, plus fixture counts and minimal KSUP1/KPREP1 source identity. It does not
parse the complete recipe or replace contact/collision certificates. Its six corruption tests plus
the combined reader's six tests run with the discovery command above. See [stress](STRESS.md),
[declared profiling protocol](PROFILING.md) and [closure](../../docs/M55-CLOSURE-ACCEPTANCE.md).

Windows-produced fixture directories can be read from WSL with these same Python commands. This
checks files actually emitted by native executables; running the Rust examples under WSL alone is
not native Windows validation. Follow the hash-verified [native runner](../windows-native/README.md)
for full native suites and add both example build/run commands to the captured runner when needed.
