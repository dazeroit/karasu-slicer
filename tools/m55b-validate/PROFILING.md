# Declared M5.5-B preliminary fixture budgets

Before measurement, declare acceptance for the owned cube and radius10mm tessellated-sphere fixtures:
<=5 seconds for one Release hollowing evaluation at up to320 source triangles and <=512MiB
whole-process peak RSS. These are preliminary fixture-specific budgets, not guarantees for every
admitted convex input. Requested bytes, allocator RSS, input setup and timed evaluation are distinct.

Build `cargo build --release -p karasu-core --example hollow_reference` outside timing. Run each of
cube and sphere subdivisions0/1/2 in three fresh processes with absent output directories:

```console
/usr/bin/time -v target/release/examples/hollow_reference /tmp/karasu-hollow-cube-1
/usr/bin/time -v target/release/examples/hollow_reference /tmp/karasu-hollow-sphere0-1 --sphere 0
/usr/bin/time -v target/release/examples/hollow_reference /tmp/karasu-hollow-sphere1-1 --sphere 1
/usr/bin/time -v target/release/examples/hollow_reference /tmp/karasu-hollow-sphere2-1 --sphere 2
```

`elapsed_ms` times geometry admission, construction, validation and certification only; original
fixture/plan generation and JSON/STL serialization are outside that interval. GNU time covers the
whole process, including them. Fixtures are synthetic and their source stamp is an asserted test
identity, not a verified user-asset digest. ASCII STL preserves round-trip f64 coordinates; geometry
JSON and exact independently checked represented volumes are the authoritative validation artifacts.

Capture source/executable hashes, rustc/OS/CPU, timing/RSS, work/requested bytes and output hashes.
Compare geometry/STL bytes across repeats; reports intentionally contain timing and must compare
all fields except elapsed_ms. Run independent readers outside timing. Host scheduling/cache/frequency
remain uncontrolled; no affinity, cold-cache, peer-slicer speed or physical-print claim is made.
Repeat native Windows tests and fixture exports separately. A host policy launch block is not a
passing performance or functional result.
