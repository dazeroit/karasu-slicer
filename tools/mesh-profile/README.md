# Mesh performance baseline

This standard-library-only runner builds the `karasu-core` example `mesh_profile` with stable Rust,
the default optimized Cargo Release profile, and no instrumentation or added dependencies. It does
not change the core, generate fixtures, install tools, or execute fuzzing. Python 3.11+ and GNU
`/usr/bin/time` are prerequisites. Read [the baseline report](../../docs/PERFORMANCE-BASELINE.md).

From the repository root, supply a new empty directory outside the repository:

```console
python3 tools/mesh-profile/run.py <evidence>/m2/m2-4c/NEW-EMPTY-SESSION
```

Build artifacts also stay in that session. Cargo runs offline. Unreviewed compiler/profile overrides
are rejected. The executable is built before measurement; compilation is not runtime. A 60-second
watchdog bounds each serial workload (180 seconds for compilation), killing its process group on
expiry. The workload stops on any unexpected nonzero status rather than increasing limits.
There is no allocator cap: inspect available memory before running. No benchmark workers overlap.

The authoritative manifest supplies fixture paths, byte sizes, hashes, faces, and vertices. FX-11
must already exist at its ignored manifest path; absence produces partial acceptance. A hash mismatch
stops the session. Binary corners are independently decoded in Python, deduplicated exactly, checked
against manifest counts and rounded source bounds, and transformed with scalar Rx -> Ry -> Rz
equations to obtain expected extrema. The Rust worker validates every measured result outside timed
sections and exits nonzero for incorrect expectations; the runner checks this rejection deliberately.
All four selected fixtures are known closed single-component construction references: E=3F/2.

Operations are file read, public buffered STL import, topology of a resident mesh, and five calls to
`TriangleMesh::world_bounds` over the complete actual vertex buffer. Import includes automatic format
detection, exact-coordinate first-occurrence deduplication, mesh validation, source bounds, retained
facet metadata, geometric and metadata diagnostics. It is not a pure parser measurement. Topology
includes its own basic diagnostic work; file reading and import are setup only for that timing.

Parameters are fixed: identity; translation (15,-7,5) mm; scale (1.25,0.8,1.1); extrinsic XYZ rotation
(0.31,-0.47,0.29) radians; combined scale and rotation about local pivot (2,-3,4) mm plus that translation.
Independent endpoint tolerance is 256*f64::EPSILON*100 mm for these sub-100-mm coordinates; dimension
tolerance is twice that because dimensions subtract two endpoints. This is not an engine epsilon.

Each timing process performs three warm-up batches and ten timed batches. Small read/import/topology
operations use 32 results per batch; small bounds use 8,192; FX-11 uses one. A preallocated vector
retains results until after the clock stops, excluding destruction and correctness checks but
including black-box barriers, Result branching, loop, and retention-push overhead. Results are
consumed, never optimized away. Small batches may have larger live data than ordinary single calls;
memory measurements deliberately use a single operation in separate processes. All samples are kept,
including outliers; per-call values are batch averages, not individual latency distributions.
Statistics are median/min/max and inclusive-quartile IQR. Warm reads reflect page-cache behavior,
not cold disk throughput. Repeated operations are independent invocations, not independent hosts.

Memory uses three new processes per workload plus three empty-worker baselines, measured with GNU
Time `-v`. Report units are KiB. Reading retains the buffer; importing retains that buffer and the
imported object. Topology/bounds import during setup, then drop the byte buffer before the measured
operation, retaining mesh, metadata, and diagnostics. Whole-process high-water RSS still includes
that setup and allocator retention. It does not establish an incremental phase allocation or exact
mesh heap size. The baseline is contextual, not a subtractable exact allocation counter. No precise
allocator instrumentation is installed. Timing and memory are deliberately separate experiments.

External evidence includes commands/statuses/stdout/stderr, source and tool provenance, build flags,
worker and runner copies/hashes (these are new uncommitted harness files), fixture hashes and scalar
expectations, raw batch CSV, JSON statistics, GNU Time reports, and an integrity manifest excluding
build intermediates. The final executable has its own hash. Preserve failures and do not silently
rerun or discard samples. Regression quality gates remain separate from benchmark acceptance.
