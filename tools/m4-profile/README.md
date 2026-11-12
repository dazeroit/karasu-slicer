# M4 LCD raster profiling

The headless `raster_reference` example exercises an owned analytic mesh through M3, M4,
and reference PNG export. These measurements are preliminary engineering acceptance budgets,
not printer calibration or commercial-reference performance comparisons.

Build once, then measure the compiled Release executable so compilation is excluded:

```console
cargo build --release -p karasu-core --example raster_reference
/usr/bin/time -v target/release/examples/raster_reference 3840 2160 4 /tmp/karasu-m4-lcd.png
/usr/bin/time -v target/release/examples/raster_reference 3840 2160 4 /tmp/karasu-m4-edges.png 1000
```

The default workload is an asymmetric triangle with a rectangular hole on a 100 x 60 mm
panel. The optional final argument selects a quantized ellipse with that many source
boundary edges (3..1000). The 1000-edge case tests edge setup and activation at the same
realistic panel resolution; its upstream M3 section normally has twice as many boundary
segments because each wall uses two triangles. Neither invocation assumes a printer model.

Repeat each measured invocation at least three times. Record executable revision/toolchain,
OS, CPU, dimensions, sample count, geometry size, `section_ms`, `raster_ms`, `png_ms`,
`payload_bytes`, and peak process resident memory. Retain stdout and timing stderr outside
production source. `raster_ms` includes M4 validation, conversion, allocation, rendering,
and statistics; it excludes M3 and PNG output. The following per-layer thresholds apply:

| Measurement | Preliminary budget |
| --- | --- |
| Raster-only elapsed time | <= 5 seconds |
| Planned simultaneous requested vector payload | <= 256 MiB |
| Whole example process peak resident memory | <= 512 MiB |

Requested payload counts edges, edge ordering and active IDs, crossings, coverage row,
output image, and compensation scratch when configured. It excludes allocator overhead,
caller-owned M3 layers, executable/runtime state, and PNG scratch. Process RSS includes
those components and is measured independently; requested payload is not an RSS promise.
The sample example uses uncompensated 256-level coverage. Morphology correctness and
radius-independent operation are tested against an independent naive grayscale oracle.

The renderer sorts E edge IDs once, maintains active edges through H*N subrows, sorts A
crossings per subrow, and accumulates integer span coverage directly into a W-element row.
Its work is O(E log E + sum(A log A + visited span pixels) + W*H), with N explicit and
bounded at 16. It never allocates a W*N by H*N image. Retained requested payload is
O(E + W + W*H). Optional rectangular grayscale compensation uses two separable monotone
queue passes, O(W*H) time independent of radius and one additional image plus O(max(W,H))
queue scratch. Layer streaming retains no raster cache; caller retention is outside that
per-call bound. All relevant loops charge deterministic work and poll cancellation.

Native Windows must be measured separately using the Release `.exe`, with the same
arguments and an explicit Windows output path. A Linux/WSL run does not validate native
Windows behavior. Windows signing-policy launch failures must be recorded as environment
failures rather than performance or compatibility passes.

Decode exported PNGs independently and compare image statistics/orientation with the
example output. Repeated runs must retain identical PNG bytes and intensity statistics.
Use SHA-256 for evidence identification; a matching hash alone does not prove correctness.
