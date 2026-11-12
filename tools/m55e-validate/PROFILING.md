# Declared M5.5-E preliminary integration budgets

The following budgets were declared before timing the frozen examples:

- Combined owned STL → hollow → drain → support reference: geometry generation/admission within
  10 seconds; whole-process peak RSS within 512 MiB, including 12 boundary PGMs and 28 reference
  PNG/machine-export layers.
- Bounded eight-support workload: decode/plan/generation within 10 seconds and whole-process peak
  RSS within 512 MiB, including two 200×200 masks, 24 convex components, one cavity and one finite cut.

Run each Release example in three fresh processes with absent output directories and GNU time `-v`:

```console
cargo build --release -p karasu-core --example preparation_reference --example preparation_stress
/usr/bin/time -v target/release/examples/preparation_reference NEW_COMBINED_DIRECTORY
/usr/bin/time -v target/release/examples/preparation_stress NEW_STRESS_DIRECTORY
```

Capture rustc/OS/CPU, executable hashes, each explicitly named timer, RSS and artifact hashes.
Combined example geometry timer excludes fixture/plan/schedule setup, M3/M4/export and serialization;
stress timer includes decoded-source and plan admission but excludes masks/output. Do not compare
these timers as identical scopes. Independent readers run outside timed processes. Host scheduling,
caches and frequency remain uncontrolled, and compilation/testing/fuzz activity may overlap.
No isolated-throughput, universal supported-plan, commercial-speed, mechanical or hardware claim.
Native Windows functional/byte comparisons are separate from repeated native profiling.

## Executed campaign

All six processes and their independent readers pass. Combined geometry takes 26.385–29.300 ms
with peak process RSS 4,864–4,984 KiB; stress takes 124.947–126.016 ms with 4,096 KiB peak RSS.
All declared fixture budgets pass. `/tmp/karasu-m55e-bench` contains environment, binary hashes,
GNU time logs, per-file manifests, independent reports and comparisons. See
[closure](../../docs/M55-CLOSURE-ACCEPTANCE.md) for per-run values and exact scope.
