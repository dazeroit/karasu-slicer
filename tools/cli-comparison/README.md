# CLI comparison suite

A fixed set of `slice` runs whose every file (print files, summaries on stdout, stderr and exit
codes) is compared byte for byte between two builds of `karasu-cli`. It guards the rule that the
command line's outputs stay identical unless a recorded decision changes them (M6.4c, TD-089).

## Contents

- `run.sh <cli> <out-dir> [fast]` runs every case into `<out-dir>/cases` and the refusals into
  `<out-dir>/errors`, printing each run's exit status and wall time. `fast` skips the slow FX-12
  case.
- `compare.sh <out-dir-a> <out-dir-b>` compares two runs and exits nonzero on any difference.
- `inputs/`: the test printers (a 4K and a large LCD, 0.05 mm layers), the resin calibrated for
  both, a plate document, a layer plan, a support document and an inside-out box. Models come
  from `fixtures/reference/stl`, including the large FX-11 and FX-12 fixtures that are not
  tracked by Git; they must be present.

The cases cover every output format family, hollowing with and without simplification, explicit
and surface drain holes, repairs, placement and pose, copies and arrangement, a plate document, a
variable layer plan, supports, antialiasing and compensation, strict filling, and the refusals
that name command-line remedies. Two cases refuse on purpose: `c15` (supports whose base join is
not below their shaft) and `c19` (a model outside the LCD area without `--center`).

## Use

Build the reference and the candidate into separate binaries, then:

```console
tools/cli-comparison/run.sh /path/to/reference-cli /tmp/cmp-ref
tools/cli-comparison/run.sh target/release/karasu-cli /tmp/cmp-new
tools/cli-comparison/compare.sh /tmp/cmp-ref /tmp/cmp-new
```

Outputs embed the input paths in their summaries, so both runs must use the same checkout
location. Keep outputs outside the repository. Wall times are indicative only; benchmarks repeat
runs on a quiet machine (see the TD-089 c5 record in `docs/TECHNOLOGY-DECISIONS.md`).

The suite's history is in [docs/TOOL-RECORDS.md](../../docs/TOOL-RECORDS.md#cli-comparison-suite-history).
