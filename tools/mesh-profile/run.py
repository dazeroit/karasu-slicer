#!/usr/bin/env python3
"""Serial, standard-library-only M2.4-C Release baseline orchestration."""

import argparse
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import signal
import statistics
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
OPERATIONS = ("read", "import", "topology", "bounds-identity", "bounds-translation",
              "bounds-scale", "bounds-rotation", "bounds-combined")


def digest(path):
    return hashlib.file_digest(path.open("rb"), "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def summary(values):
    quartiles = statistics.quantiles(values, n=4, method="inclusive")
    return dict(median=statistics.median(values), minimum=min(values), maximum=max(values),
                iqr=quartiles[2] - quartiles[0])


def scalar_bounds(vertices, operation):
    """Independent scalar Rx -> Ry -> Rz; never calls the production transform/bounds code."""
    offset = (15., -7., 5.) if operation in ("bounds-translation", "bounds-combined") else (0., 0., 0.)
    scale = (1.25, .8, 1.1) if operation in ("bounds-scale", "bounds-combined") else (1., 1., 1.)
    pivot = (2., -3., 4.) if operation == "bounds-combined" else (0., 0., 0.)
    angles = (.31, -.47, .29) if operation in ("bounds-rotation", "bounds-combined") else (0., 0., 0.)
    (sx, cx), (sy, cy), (sz, cz) = [(math.sin(a), math.cos(a)) for a in angles]
    low, high = [math.inf] * 3, [-math.inf] * 3
    for vertex in vertices:
        x, y, z = [(v - c) * s for v, c, s in zip(vertex, pivot, scale)]
        y, z = cx * y - sx * z, sx * y + cx * z
        x, z = cy * x + sy * z, -sy * x + cy * z
        x, y = cz * x - sz * y, sz * x + cz * y
        point = [v + c + t for v, c, t in zip((x, y, z), pivot, offset)]
        low = [min(a, b) for a, b in zip(low, point)]
        high = [max(a, b) for a, b in zip(high, point)]
    return low + high


class Session:
    def __init__(self, path):
        self.path = path.resolve()
        self.path.mkdir(parents=True, exist_ok=True)
        if any(self.path.iterdir()):
            raise ValueError("evidence directory must be empty")
        self.commands = []
        self.env = dict(os.environ, LC_ALL="C", CARGO_TARGET_DIR=str(self.path / "target"),
                        CARGO_NET_OFFLINE="true")
        # Do not silently accept flags that would make this a different build profile.
        unexpected = [k for k in self.env if k.startswith(("CARGO_PROFILE_", "CARGO_ENCODED_RUSTFLAGS"))
                      or k in ("RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC")]
        if unexpected:
            raise ValueError(f"unreviewed build overrides: {unexpected}")

    def run(self, name, command, timeout=60, expected=0):
        started = time.time()
        process = subprocess.Popen(command, cwd=ROOT, env=self.env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, start_new_session=True)
        expired = False
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            expired = True
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
        (self.path / f"{name}.stdout").write_text(stdout, encoding="utf-8")
        (self.path / f"{name}.stderr").write_text(stderr, encoding="utf-8")
        self.commands.append(dict(name=name, argv=list(map(str, command)), exit_code=process.returncode,
                                  timeout=expired, elapsed_seconds=time.time() - started,
                                  expected_exit=expected))
        write_json(self.path / "commands.json", self.commands)
        if expired or process.returncode != expected:
            raise RuntimeError(f"{name}: exit {process.returncode}, timeout={expired}")
        return stdout


def fixture_data(session):
    manifest_path = ROOT / "fixtures/reference/manifest.json"
    manifest = json.loads(manifest_path.read_text())
    fixtures, missing = [], []
    for entry in manifest["fixtures"]:
        if entry["fixture_id"] not in ("FX-01", "FX-02", "FX-05", "FX-11"):
            continue
        path = ROOT / "fixtures/reference" / entry["filename"]
        if not path.is_file():
            missing.append(entry["fixture_id"])
            continue
        if digest(path) != entry["sha256"] or path.stat().st_size != entry["file_size_bytes"]:
            raise ValueError(f"unverified fixture: {path}")
        data = path.read_bytes()
        count = struct.unpack_from("<I", data, 80)[0]
        assert len(data) == 84 + 50 * count and count == entry["triangle_count"]
        vertices = set()
        for index in range(count):
            for corner in range(3):
                vertices.add(struct.unpack_from("<3f", data, 84 + 50 * index + 12 + 12 * corner))
        assert len(vertices) == entry["unique_vertex_count"]
        bounds = {op: scalar_bounds(vertices, op) for op in OPERATIONS[3:]}
        recorded = entry["bounding_box_mm"]["min"] + entry["bounding_box_mm"]["max"]
        assert all(abs(a - b) <= 5.1e-10 for a, b in zip(bounds["bounds-identity"], recorded))
        fixtures.append(dict(id=entry["fixture_id"], path=str(path), sha256=digest(path),
                             bytes=len(data), vertices=len(vertices), faces=count,
                             edges=3 * count // 2, bounds=bounds))
    write_json(session.path / "fixtures.json", dict(manifest_sha256=digest(manifest_path),
               fixtures=fixtures, missing=missing))
    return fixtures, missing


def worker_args(executable, fixture, operation, mode, batch=1):
    expected = fixture["bounds"].get(operation, fixture["bounds"]["bounds-identity"])
    return [str(executable), mode, operation, fixture["path"], str(fixture["vertices"]),
            str(fixture["faces"]), str(fixture["edges"]), str(fixture["bytes"]), str(batch),
            *map(repr, expected)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path, help="empty directory outside the repository")
    args = parser.parse_args()
    if args.evidence.resolve().is_relative_to(ROOT):
        parser.error("evidence must be outside Git")
    session = Session(args.evidence)
    provenance = {"commit": session.run("head", ["git", "rev-parse", "HEAD"]).strip(),
                  "branch": session.run("branch", ["git", "branch", "--show-current"]).strip(),
                  "status": session.run("initial-status", ["git", "status", "--short"]),
                  "build_profile": "Cargo default release: opt-level=3, no LTO/instrumentation",
                  "environment": {k: v for k, v in session.env.items()
                                  if k.startswith(("CARGO_", "RUST"))},
                  "warmup_batches": 3, "samples": 10, "memory_runs": 3,
                  "workload_timeout_seconds": 60, "parallel_workers": 1}
    for name, command in (("rustc", ["rustc", "+stable", "-Vv"]),
                          ("cargo", ["cargo", "+stable", "-V"]),
                          ("kernel", ["uname", "-a"]), ("cpu", ["lscpu"]),
                          ("memory", ["free", "-b"]), ("load-before", ["uptime"]),
                          ("time-version", ["/usr/bin/time", "--version"])):
        provenance[name] = session.run(name, command)
    sources = (Path(__file__).resolve(), ROOT / "crates/karasu-core/examples/mesh_profile.rs",
               ROOT / "tools/mesh-profile/test_run.py", ROOT / "tools/mesh-profile/README.md")
    provenance["source_hashes"] = {str(p.relative_to(ROOT)): digest(p) for p in
                                  (*sources, ROOT / "Cargo.toml", ROOT / "Cargo.lock")}
    write_json(session.path / "provenance.json", provenance)
    # Retain patches because the newly added worker is not part of the production baseline commit.
    session.run("tracked-source-diff", ["git", "diff", "--binary"])
    for source in sources:
        (session.path / source.name).write_bytes(source.read_bytes())
    fixtures, missing = fixture_data(session)
    session.run("release-build", ["cargo", "+stable", "build", "--release", "--offline",
                                  "-p", "karasu-core", "--example", "mesh_profile", "-vv"], timeout=180)
    executable = session.path / "target/release/examples/mesh_profile"
    write_json(session.path / "executable.json", dict(path=str(executable), sha256=digest(executable)))
    session.run("worker-baseline", [str(executable), "baseline"])
    # An intentional rejection verifies a real nonzero result on invalid expectations.
    negative = worker_args(executable, fixtures[0], "import", "memory")
    negative[4] = str(fixtures[0]["vertices"] + 1)
    session.run("expected-count-rejection", negative, expected=1)
    negative_bounds = worker_args(executable, fixtures[0], "bounds-combined", "memory")
    negative_bounds[9] = str(float(negative_bounds[9]) + 1.0)
    session.run("expected-bounds-rejection", negative_bounds, expected=1)
    negative_topology = worker_args(executable, fixtures[0], "topology", "memory")
    negative_topology[6] = str(fixtures[0]["edges"] + 1)
    session.run("expected-topology-rejection", negative_topology, expected=1)
    timing, memory = [], []
    for fixture in fixtures:
        for operation in OPERATIONS:
            batch = 1 if fixture["id"] == "FX-11" else (8192 if operation.startswith("bounds-") else 32)
            name = f'{fixture["id"]}-{operation}'
            stdout = session.run(name + "-timing", worker_args(executable, fixture, operation, "timing", batch))
            rows = list(csv.DictReader(io.StringIO(stdout)))
            assert len(rows) == 10 and all(int(row["batch"]) == batch for row in rows)
            samples = [int(row["total_ns"]) / batch for row in rows]
            record = dict(fixture=fixture["id"], operation=operation, batch=batch,
                          samples_ns=samples, statistics_ns=summary(samples), validation="PASS")
            if operation in ("read", "import"):
                record["median_mib_per_second"] = fixture["bytes"] / 2**20 / (statistics.median(samples) / 1e9)
            timing.append(record)
            write_json(session.path / "timing.json", timing)
            print(f'{name}: median {statistics.median(samples) / 1e6:.6f} ms', flush=True)
    workloads = [("baseline", [str(executable), "baseline"])] + [
        (f'{fixture["id"]}-{op}', worker_args(executable, fixture, op, "memory"))
        for fixture in fixtures for op in OPERATIONS]
    for name, command in workloads:
        peaks = []
        for run in range(3):
            report = session.path / f"{name}-rss-{run}.time"
            session.run(f"{name}-rss-{run}", ["/usr/bin/time", "-v", "-o", str(report), *command])
            line = next(line for line in report.read_text().splitlines() if "Maximum resident set size" in line)
            peaks.append(int(line.split(":")[-1]))
        memory.append(dict(workload=name, runs=3, peak_rss_kib=peaks, statistics_kib=summary(peaks)))
        write_json(session.path / "memory.json", memory)
    session.run("load-after", ["uptime"])
    write_json(session.path / "result.json", dict(acceptance="PARTIAL" if missing else "PASS",
               missing_fixtures=missing, timing_workloads=len(timing), memory_workloads=len(memory),
               unexpected_nonzero_exits=[], limitations=["warm page cache; not cold storage latency",
               "WSL shared host, no CPU pinning or frequency isolation",
               "whole-process RSS includes setup and allocator retention, not phase heap accounting",
               "timing includes loop/black_box/Result/retention push, excludes result drop/validation"]))
    integrity(session.path)
    print(f"Evidence: {session.path}", flush=True)


def integrity(path):
    records = {str(p.relative_to(path)): digest(p) for p in sorted(path.rglob("*"))
               if p.is_file() and "target" not in p.relative_to(path).parts and p.name != "integrity.json"}
    write_json(path / "integrity.json", records)


if __name__ == "__main__":
    main()
