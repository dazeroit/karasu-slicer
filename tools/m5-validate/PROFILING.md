# M5 preliminary streaming budgets

Declare before measuring: the complete M3/M4/M5 sample path must average <=5 seconds per layer on
the specified P1-size job, use <=512 MiB whole-process peak RSS, and retain no job-sized mask stack.
Adapter scratch, source masks, allocator overhead and RSS are different quantities; the default
8-MiB metadata, 256-MiB encoded-layer and classic 4-GiB file limits remain independent admission
policies. These preliminary measurements are not universal latency or memory guarantees.

Build `cargo build --release -p karasu-core --example print_reference` once outside timing. Run the
compiled example in three fresh processes for each case, choosing previously absent destinations:

```console
/usr/bin/time -v target/release/examples/print_reference /tmp/karasu-m5-small-1
/usr/bin/time -v target/release/examples/print_reference /tmp/karasu-m5-many-1 2000
/usr/bin/time -v target/release/examples/print_reference /tmp/karasu-m5-p1-1 --p1 3
```

The small/many cases use 64x40 pixels and generate CTB v2 plain, GOO v3, firmware-motion Photon v1,
PP1 ZIP and reference PNG/JSON. They test metadata growth and source exhaustion through 2,000 layers.
The P1-size case uses 13,312x5,120 pixels (65 MiPixels), single full-frame PP1 and reference output,
without retaining preceding masks. Physical dimensions 223x126x230 mm come from the public Anycubic
manual; origin/orientation and resin values are explicitly uncalibrated fixture choices. No
physical-print, dual-material, sensor, cloud or firmware-motion interpretation is validated by timing.

Each reported exporter time includes regenerated M3 geometry, M4 masks, validation, encoding,
filesystem writes/sync and create-only publication. It excludes initial fixture/job construction.
Whole-process RSS additionally includes that setup and runtime state. PNG stored DEFLATE prioritizes
bounded deterministic output over disk compression. Do not compare these observations with another
slicer's throughput or treat nominal estimated print time as benchmark wall time.

Capture rustc/OS/CPU, source and executable hashes, stdout, GNU-time stderr, output sizes and exact
artifact hashes. Other host activity, CPU frequency, filesystem caching and scheduling are uncontrolled;
no affinity, cold-cache or allocator-attribution claim is made. Validate completed output with the
independent Python readers and third-party format readers outside the timed interval. Repeat native
Windows functional export separately; Linux/WSL performance does not establish native behavior.
