# Independent M5.5-C mask validation

Generate this synthetic fixture only after declaring the applicable profiling budget:

```console
cargo run --release -p karasu-core --example drain_reference -- /tmp/karasu-drain-output
python3 tools/m55c-validate/validate_masks.py /tmp/karasu-drain-output
python3 -m unittest discover -s tools/m55c-validate -v
```

The output directory must not already exist. The example evaluates a 20 mm cube,
2 mm hollowing offset, and one diameter-2 mm cut from `(10,10,-1)` towards positive
Z for 4 mm. The requested approximation error is 0.001 mm. It writes four canonical
binary PGM masks at 200 × 200 pixels over `[0,20]²`, with one center sample per pixel,
no antialiasing and no compensation. These are analytic software fixtures, not
calibrated printer files. The source stamp is a synthetic caller assertion.

`elapsed_ms` covers hollowing, drainage admission, four actual material-difference
layer evaluations and rasterization. Fixture, recipe and schedule creation, JSON
serialization, filesystem publication and independent reading are excluded. PGM
bytes and the plan identity are suitable for cross-platform comparison; elapsed
time is intentionally variable. The reader computes SHA-256 for each complete
PGM and the decoded canonical plan record.

The independent standard-library Python reader uses exact rational pixel centers
and squared distances. At Z=1, it requires void inside radius `1 - radial_error`
and material outside radius `1 + radial_error`; pixels in the certified error
annulus may have either binary value. At Z=2.5 and Z=3, it checks the square cavity
`(2,18)²`. Z=3 exercises the terminal cutter cap entirely inside that cavity. At
Z=19, the top wall is entirely material because the cavity ends at Z=18. Pixel
centers are separated from the square boundaries by 0.05 mm, exceeding the
permitted 0.001 mm hollowing error.

The reader checks all 160,000 pixels, apart from any explicitly reported circle
error annulus. It rejects unexpected fixture settings, changed planes, malformed
PGM headers/lengths, nonbinary intensities and excessive error certificates. It
does not replace the Rust geometric certificate, prove arbitrary CSG topology,
validate physical drainage or establish printer compatibility. Eight reader unit
tests exercise ideal analytic acceptance and intentional corruptions independently
of the Rust executable.
