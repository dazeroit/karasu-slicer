# Fuzz targets

Coverage-guided fuzz targets (cargo-fuzz, libFuzzer) for karasu-slicer's parsers and geometry
stages, in a testing-only package isolated from the production crates.

**Targets** (`fuzz_targets/`): `binary_stl`, `ascii_stl`, `auto_stl`, `topology`,
`layer_schedule`, `layer_engine`, `construct_material`, `strict_regions`, `profile_json`,
`hollow_mesh`, `drainage`, `supports` and `preparation_session`.

**Tools** (`tools/`):
- `prepare_corpus.py` writes the seed corpora into a new directory outside the repository;
- `check_preparation.py` replays those seeds through the oracles with the stable toolchain
  (ordinary fixed-input validation, not fuzzing);
- `run_campaign.py` runs a bounded campaign with an installed nightly toolchain;
- `replay.rs` replays a target's seeds or one input without instrumentation:
  `replay TARGET SEED_DIRECTORY_OR_FILE`.

## Prerequisites

A dated nightly Rust toolchain and `cargo-fuzz`, installed beforehand (the tools never install
anything), on WSL/Linux. Keep every run in a new directory outside the repository.

## Running

```bash
run_dir=/absolute/path/outside/the/repository/NEW-RUN
python3 -B fuzz/tools/prepare_corpus.py --output "$run_dir"
python3 -B fuzz/tools/check_preparation.py --output "$run_dir"
python3 -B fuzz/tools/run_campaign.py --output "$run_dir" --toolchain nightly-YYYY-MM-DD
```

One target by hand, for example:

```bash
cargo +INSTALLED_NIGHTLY fuzz run TARGET \
  --fuzz-dir fuzz --target-dir "$run_dir/target" --sanitizer address --debug-assertions \
  "$run_dir/corpus/TARGET" "$run_dir/seeds/TARGET" -- \
  -max_total_time=120 -timeout=3 -rss_limit_mb=1024 -print_final_stats=1 \
  -artifact_prefix="$run_dir/artifacts/TARGET/"
```

Reproduce a preserved failure without mutation (`-runs=1`), and minimize it into a new file
(`-minimize_crash=1 -exact_artifact_path=...`), never over the original.

Each target's input encoding, oracle and limits, the exact campaign commands, the failure
workflow and every campaign's results are in
[docs/FUZZ-CAMPAIGNS.md](../docs/FUZZ-CAMPAIGNS.md).
