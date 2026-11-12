# Independent hollow fixture validation

Generate an absent output directory and check the represented result:

```console
cargo run --release -p karasu-core --example hollow_reference -- /tmp/karasu-hollow-cube
python3 tools/m55b-validate/validate_hollow.py /tmp/karasu-hollow-cube --cube
python3 -m unittest discover -s tools/m55b-validate -p 'test_*.py'
```

For tessellated spheres append `--sphere 0`, `--sphere 1` or `--sphere 2` to the example command
and omit `--cube` from the reader command. Use a fresh output directory each time.

The reader uses Python standard-library exact rational arithmetic, independently of Karasu code.
It checks represented mesh orientation/edge closure/component counts, convexity, source preservation,
shell assembly, volume intervals and the source-plane distance bound. It does not independently
prove reverse erosion inclusion, vertex-link validity, every coplanar contact or printability.
JSON geometry is authoritative; reports include variable timing. ASCII STL preserves round-trip
f64 coordinates. Compare geometry/STL bytes and report fields except elapsed time across platforms.

See [profiling procedure](PROFILING.md) and [results](../../docs/M55B-VALIDATION.md) for timing scope,
measured budgets, native validation provenance and limitations.
