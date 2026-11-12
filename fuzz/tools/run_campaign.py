#!/usr/bin/env python3
"""Record prerequisites; run bounded nightly libFuzzer only when already installed. Never install."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import signal
import subprocess
import time

from prepare_corpus import ROOT, TARGETS, outside_repository, verify_fixtures


def probe(command):
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=30)
        return {"command": command, "exit_code": result.returncode,
                "stdout": result.stdout, "stderr": result.stderr}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": command, "exit_code": None, "error": str(error)}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def corpus_size(directory):
    files = [path for path in directory.iterdir() if path.is_file()]
    return {"files": len(files), "bytes": sum(path.stat().st_size for path in files)}


def event_metrics(text, event):
    lines = re.findall(r"^#\d+\s+" + event + r"\b[^\n]*", text, re.MULTILINE)
    if not lines:
        return None
    line = lines[-1]
    metrics = {"reported_line": line}
    for name, pattern in [("coverage", r"\bcov:\s*(\d+)"), ("features", r"\bft:\s*(\d+)"),
                          ("corpus_inputs", r"\bcorp:\s*(\d+)")]:
        match = re.search(pattern, line)
        metrics[name] = int(match.group(1)) if match else None
    return metrics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=outside_repository)
    parser.add_argument("--toolchain", default="nightly", help="already installed nightly name, preferably dated")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    report_path = args.output / "campaign.json"
    if report_path.exists():
        raise ValueError("campaign.json already exists; use a fresh run directory")
    commands = [["rustc", "-vV"], ["cargo", "-V"], ["rustup", "toolchain", "list"],
                ["rustup", "show", "active-toolchain"], ["cargo", "fuzz", "--version"],
                ["git", "rev-parse", "HEAD"], ["git", "status", "--porcelain=v1", "--untracked-files=all"]]
    probes = [probe(command) for command in commands]
    compiler = shutil.which(os.environ.get("CXX", "g++")) or shutil.which("c++")
    if compiler:
        probes.append(probe([compiler, "--version"]))
    names = [line.split()[0] for line in probes[2].get("stdout", "").splitlines() if line.strip()]
    host_match = re.search(r"^host: (.+)$", probes[0].get("stdout", ""), re.MULTILINE)
    host = host_match.group(1) if host_match else ""
    desired = args.toolchain if args.toolchain in names else args.toolchain + "-" + host
    selected = desired if desired in names and desired.startswith("nightly-") else None
    if selected:
        probes.extend([probe(["rustc", "+" + selected, "-vV"]), probe(["cargo", "+" + selected, "-V"])])
    missing = []
    if selected is None:
        missing.append(f"installed nightly Rust toolchain matching {args.toolchain}")
    if probes[4].get("exit_code") != 0 or not shutil.which("cargo-fuzz"):
        missing.append("cargo-fuzz executable")
    if compiler is None:
        missing.append("C++11-capable compiler")
    source_paths = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=ROOT
    ).decode().split("\0")
    source_hashes = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
                     for name in sorted(set(source_paths)) if name and (ROOT / name).is_file()}
    prerequisites = {"timestamp_utc": datetime.now(timezone.utc).isoformat(), "platform": platform.platform(),
                     "probes": probes, "selected_nightly": selected, "cxx": compiler, "missing": missing,
                     "memory": Path("/proc/meminfo").read_text() if Path("/proc/meminfo").exists() else None,
                     "source_sha256": source_hashes}
    write_json(args.output / "prerequisites.json", prerequisites)
    # Record zero *coverage-guided* executions separately from any ordinary seed replay.
    report = {"status": "BLOCKED" if missing else "PENDING", "missing": missing,
              "targets": {target: {"executions": 0, "coverage": None, "status": "NOT_RUN"} for target in TARGETS}}
    write_json(report_path, report)
    if missing:
        print("BLOCKED: " + "; ".join(missing), flush=True)
        return 2
    if not (args.output / "seed-manifest.json").is_file():
        raise ValueError("prepare seeds in the same output directory first")
    seed_manifest = json.loads((args.output / "seed-manifest.json").read_text())
    verify_fixtures()
    for seed in seed_manifest["seeds"]:
        raw = (args.output / "seeds" / seed["target"] / seed["name"]).read_bytes()
        if hashlib.sha256(raw).hexdigest() != seed["sha256"]:
            raise ValueError("prepared seed hash mismatch")
    (args.output / "logs").mkdir(exist_ok=True)
    env = dict(os.environ, CXX=compiler, CARGO_TARGET_DIR=str(args.output / "target"))
    for index, target in enumerate(TARGETS):
        corpus = args.output / "corpus" / target
        artifacts = args.output / "artifacts" / target
        corpus.mkdir(parents=True, exist_ok=True)
        artifacts.mkdir(parents=True, exist_ok=True)
        initial_files = {"discoveries": corpus_size(corpus), "seeds": corpus_size(args.output / "seeds" / target)}
        command = ["cargo", "+" + selected, "fuzz", "run", target,
                   "--fuzz-dir", str(ROOT / "fuzz"), "--target-dir", str(args.output / "target"),
                   "--sanitizer", "address", "--debug-assertions", str(corpus),
                   str(args.output / "seeds" / target), "--", "-max_len=65536", "-timeout=3",
                   "-rss_limit_mb=1024", "-malloc_limit_mb=256", "-max_total_time=120",
                   f"-seed={20261015 + index}", "-print_final_stats=1", "-use_value_profile=1",
                   "-artifact_prefix=" + str(artifacts) + "/"]
        log_path = args.output / "logs" / (target + ".log")
        with log_path.open("x", encoding="utf-8") as log:
            log.write("COMMAND=" + shlex.join(command) + "\n")
            log.flush()
            print("Running " + target, flush=True)
            started = time.monotonic()
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            watchdog = False
            try:
                code = process.wait(timeout=900)  # Includes compilation; fuzzer itself gets 120 s.
            except subprocess.TimeoutExpired:
                watchdog = True
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    code = process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    code = process.wait()
            log.write(f"\nEXIT_CODE={code}\nWATCHDOG={watchdog}\n")
        text = log_path.read_text()
        executions = re.findall(r"stat::number_of_executed_units:\s*(\d+)", text)
        coverage = re.findall(r"\bcov:\s*(\d+)", text)
        done = bool(re.search(r"^#\d+\s+DONE\b", text, re.MULTILINE))
        findings = {
            "sanitizer": re.findall(r"^.*(?:ERROR: (?:AddressSanitizer|LeakSanitizer|UndefinedBehaviorSanitizer)|SUMMARY: .*Sanitizer|runtime error:).*$", text, re.MULTILINE),
            "timeout": re.findall(r"^.*ERROR: libFuzzer: timeout.*$", text, re.MULTILINE),
            "resource": re.findall(r"^.*ERROR:.*(?:out-of-memory|out of memory|rss limit|malloc limit).*$", text, re.MULTILINE | re.IGNORECASE),
            "panic_or_signal": re.findall(r"^.*(?:panicked at|ERROR: libFuzzer: deadly signal).*$", text, re.MULTILINE),
            "unsupported_flags": re.findall(r"^.*WARNING:.*(?:unrecognized|unknown).*flag.*$", text, re.MULTILINE),
        }
        artifacts_found = sorted(str(path) for path in artifacts.iterdir())
        runtime = re.findall(r"Done \d+ runs in (\d+) second", text)
        statistics = {name: int(value) for name, value in re.findall(r"^stat::([a-z_]+):\s*(\d+)", text, re.MULTILINE)}
        failure = code != 0 or watchdog or not done or not executions or artifacts_found or any(findings.values())
        report["targets"][target] = {"command": command, "exit_code": code, "watchdog": watchdog,
                                     "status": "NEEDS_INVESTIGATION" if failure else "BOUNDED_RUN_COMPLETE",
                                     "executions": int(executions[-1]) if executions else None,
                                     "coverage": int(coverage[-1]) if coverage else None,
                                     "initial_metrics": event_metrics(text, "INITED"),
                                     "final_metrics": event_metrics(text, "DONE"),
                                     "initial_filesystem_corpus": initial_files,
                                     "final_filesystem_corpus": {"discoveries": corpus_size(corpus), "seeds": corpus_size(args.output / "seeds" / target)},
                                     "elapsed_including_build_seconds": time.monotonic() - started,
                                     "reported_runtime_seconds": int(runtime[-1]) if runtime else None,
                                     "statistics": statistics, "findings": findings, "artifacts": artifacts_found}
        report["status"] = "PARTIAL" if failure else "RUNNING"
        write_json(report_path, report)
        lock = ROOT / "fuzz/Cargo.lock"
        if lock.exists():
            shutil.copyfile(lock, args.output / "fuzz-Cargo.lock")
        if failure:
            print(f"Preserved {target} evidence; investigate before continuing", flush=True)
            return 1
    report["status"] = "BOUNDED_RUNS_COMPLETE_REVIEW_REQUIRED"
    write_json(report_path, report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
