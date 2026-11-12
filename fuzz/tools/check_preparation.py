#!/usr/bin/env python3
"""Validate fuzz oracles and replay seeds with stable Rust; this is never a fuzz campaign."""

import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess

from prepare_corpus import ROOT, TARGETS, outside_repository, verify_fixtures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=outside_repository)
    args = parser.parse_args()
    directory = args.output / "preparation-check"
    directory.mkdir(exist_ok=False)
    records = []

    def run(name, command, extra_env=None):
        environment = dict(os.environ)
        environment.update(extra_env or {})
        result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True,
                                text=True, timeout=60)
        log = directory / (name + ".log")
        log.write_text("COMMAND=" + shlex.join([str(arg) for arg in command]) + "\n" +
                       result.stdout + result.stderr + f"\nEXIT_CODE={result.returncode}\n", encoding="utf-8")
        records.append({"name": name, "command": [str(arg) for arg in command],
                        "environment_overrides": extra_env or {}, "exit_code": result.returncode})
        (directory / "commands.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")
        print(f"{name}: exit {result.returncode}", flush=True)
        if result.returncode != 0:
            raise RuntimeError(f"preparation check failed: {name}; see {log}")
        return result.stdout

    for script in (ROOT / "fuzz/tools").glob("*.py"):
        ast.parse(script.read_text(), filename=str(script))
    verify_fixtures()
    seed_manifest = json.loads((args.output / "seed-manifest.json").read_text())
    for seed in seed_manifest["seeds"]:
        path = args.output / "seeds" / seed["target"] / seed["name"]
        if hashlib.sha256(path.read_bytes()).hexdigest() != seed["sha256"]:
            raise ValueError(f"seed hash mismatch: {path}")
    metadata = json.loads(run("metadata", ["cargo", "metadata", "--offline", "--no-deps", "--format-version=1"]))
    if len(metadata["workspace_members"]) != 3 or any(p["name"] == "libfuzzer-sys" for p in metadata["packages"]):
        raise ValueError("production workspace dependency boundary changed")
    run("stable-fmt", ["cargo", "fmt", "--all", "--", "--check"])
    run("fuzz-fmt", ["cargo", "fmt", "--manifest-path", "fuzz/Cargo.toml", "--all", "--", "--check"])
    run("replay-fmt", ["rustfmt", "--edition=2021", "--check", "fuzz/tools/replay.rs"])
    run("stable-clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
    run("stable-test", ["cargo", "test", "--workspace"])
    run("stable-build", ["cargo", "build", "--workspace"])
    run("stable-doc", ["cargo", "doc", "--workspace", "--no-deps", "--document-private-items"],
        {"RUSTDOCFLAGS": "-D warnings"})
    run("diff-check", ["git", "diff", "--check"])
    target = Path(metadata["target_directory"]) / "debug"
    core = "karasu_core=" + str(target / "libkarasu_core.rlib")
    common = ["--edition=2021", "--crate-type=lib", "--crate-name=karasu_mesh_fuzz", "fuzz/src/lib.rs",
              "--extern", core, "-L", "dependency=" + str(target / "deps"), "-D", "warnings"]
    run("oracle-compile", ["rustc", *common, "--out-dir", directory])
    if not shutil.which("clippy-driver"):
        raise RuntimeError("clippy-driver is required for the oracle lint check")
    run("oracle-clippy", ["clippy-driver", *common, "--emit=metadata", "-o", directory / "oracle.rmeta",
                          "-W", "clippy::all"])
    run("replay-compile", ["rustc", "--edition=2021", "fuzz/tools/replay.rs", "--extern",
                           "karasu_mesh_fuzz=" + str(directory / "libkarasu_mesh_fuzz.rlib"), "-L",
                           "dependency=" + str(target / "deps"), "-D", "warnings", "-o", directory / "replay"])
    for name in TARGETS:
        run("seed-replay-" + name, [directory / "replay", name, args.output / "seeds" / name])
    summary = {"status": "PREPARATION_CHECKED_NOT_FUZZED", "seeds": seed_manifest["counts"],
               "coverage_guided_executions": 0, "coverage": None,
               "launcher_macro_compilation": "NOT_PERFORMED_BY_PREPARATION_CHECK",
               "source_head": seed_manifest["head"]}
    (directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary), flush=True)


if __name__ == "__main__":
    main()
