#!/usr/bin/env python3
"""Verify owned fixtures, then create fresh external seeds and a provenance manifest."""

import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "fixtures/reference"
TARGETS = ("binary_stl", "ascii_stl", "auto_stl", "topology")
SELECTED = {("FX-01", None), ("FX-02", None), ("FX-05", None),
            ("FX-08", None), ("FX-09", None), ("FX-10", "A"), ("FX-10", "B")}


def outside_repository(path):
    """Reject repository paths and ancestors rather than risking generated data in Git."""
    path = Path(path).expanduser().resolve()
    if path == ROOT or ROOT in path.parents or path in ROOT.parents:
        raise ValueError("output must be outside the repository and its ancestors")
    return path


def digest(data):
    return hashlib.sha256(data).hexdigest()


def verify_fixtures():
    manifest_bytes = (FIXTURES / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    records = []
    selected = []
    for entry in manifest["fixtures"]:
        if entry["fixture_id"] == "FX-11":
            continue  # Optional large file is never read, generated, or seeded by this tool.
        path = FIXTURES / entry["filename"]
        subprocess.run(["git", "ls-files", "--error-unmatch", "--", str(path.relative_to(ROOT))],
                       cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        raw = path.read_bytes()
        if digest(raw) != entry["sha256"]:
            raise ValueError(f"canonical fixture hash mismatch: {entry['filename']}")
        records.append({"filename": entry["filename"], "sha256": digest(raw), "bytes": len(raw)})
        if (entry["fixture_id"], entry["variant"]) in SELECTED:
            selected.append((entry, raw))
    if len(selected) != 7:
        raise ValueError("required seven seed fixtures were not found")
    return records, selected, digest(manifest_bytes)


def indexed_stl(raw):
    count = struct.unpack_from("<I", raw, 80)[0]
    if len(raw) != 84 + 50 * count:
        raise ValueError("reference binary STL record length mismatch")
    vertices, faces, identities = [], [], {}
    for face in range(count):
        indices = []
        for corner in range(3):
            point = struct.unpack_from("<3f", raw, 84 + 50 * face + 12 + 12 * corner)
            if point not in identities:
                identities[point] = len(vertices)
                vertices.append(point)
            indices.append(identities[point])
        faces.append(indices)
    return vertices, faces


def topology_bytes(vertices, faces):
    if len(vertices) > 512 or len(faces) > 1024 or (faces and not vertices):
        raise ValueError("topology seed exceeds harness caps")
    raw = bytearray(struct.pack("<HH", len(vertices), len(faces)))
    for point in vertices:
        scaled = [round(value * 16) for value in point]
        if any(value < -32768 or value > 32767 for value in scaled):
            raise ValueError("seed coordinate exceeds the documented i16 encoding")
        raw.extend(struct.pack("<3h", *scaled))
    for face in faces:
        if any(index < 0 or index >= len(vertices) for index in face):
            raise ValueError("invalid synthetic seed index")
        raw.extend(struct.pack("<3H", *face))
    return bytes(raw)


def ascii_stl(vertices, faces):
    lines = ["solid owned-seed"]
    for face in faces:
        lines.extend(["facet normal 0 0 0", "outer loop"])
        for vertex in face:
            lines.append("vertex " + " ".join(format(value, ".17g") for value in vertices[vertex]))
        lines.extend(["endloop", "endfacet"])
    lines.append("endsolid owned-seed")
    return ("\n".join(lines) + "\n").encode("ascii")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=outside_repository)
    args = parser.parse_args()
    fixtures, selected, manifest_hash = verify_fixtures()  # Verify all before any seed copy.
    seeds = args.output / "seeds"
    seeds.mkdir(parents=True, exist_ok=False)  # Never overwrite a previous seed corpus.
    for target in TARGETS:
        (seeds / target).mkdir()
    records = []

    def emit(target, name, raw, origin):
        if len(raw) > 65536:
            raise ValueError(f"seed exceeds 64 KiB: {name}")
        path = seeds / target / name
        with path.open("xb") as stream:
            stream.write(raw)
        records.append({"target": target, "name": name, "bytes": len(raw),
                        "sha256": digest(raw), "origin": origin})

    for entry, raw in selected:
        name = entry["fixture_id"].lower() + (entry["variant"] or "").lower()
        emit("binary_stl", f"accept-{name}", raw, entry["filename"])
        emit("auto_stl", f"accept-{name}", raw, entry["filename"])
        vertices, faces = indexed_stl(raw)
        emit("topology", name, topology_bytes(vertices, faces),
             entry["filename"] + "; first-occurrence indices, coordinates quantized to 1/16 mm")
        # The cylinder ASCII encoding is intentionally omitted to keep seeds small and below 64 KiB.
        if entry["fixture_id"] != "FX-02":
            text = ascii_stl(vertices, faces)
            emit("ascii_stl", f"accept-{name}", text, entry["filename"] + "; independent ASCII encoding")
            emit("auto_stl", f"accept-{name}-ascii", text, entry["filename"] + "; ASCII encoding")

    cube = next(raw for entry, raw in selected if entry["fixture_id"] == "FX-01")
    binary_cases = {
        "accept-empty-binary": bytes(84),
        "accept-solid-header": b"solid" + cube[5:],
        "reject-empty-input": b"", "reject-short-header": b"solid",
        "reject-truncated-record": cube[:-1], "reject-trailing-byte": cube + b"\n",
        "reject-excessive-count": cube[:80] + struct.pack("<I", 0xFFFFFFFF) + cube[84:],
    }
    for label, bits in [("nan", 0x7FC00001), ("infinity", 0x7F800000)]:
        bad = bytearray(cube)
        struct.pack_into("<I", bad, 96, bits)
        binary_cases[f"reject-{label}-vertex"] = bytes(bad)
        stored = bytearray(cube)
        struct.pack_into("<I", stored, 84, bits)
        binary_cases[f"accept-{label}-normal"] = bytes(stored)
    attributes = bytearray(cube)
    struct.pack_into("<H", attributes, 132, 65535)
    binary_cases["accept-opaque-attribute"] = bytes(attributes)
    for name, raw in binary_cases.items():
        for target in ["binary_stl", "auto_stl"]:
            emit(target, name, raw, "synthetic binary boundary/metadata case")

    triangle = ascii_stl([(0, 0, 0), (1, 0, 0), (0, 1, 0)], [[0, 1, 2]])
    ascii_cases = {
        "accept-empty-solid": b"solid\nendsolid\n",
        "accept-triangle": triangle,
        "accept-scientific-whitespace": triangle.replace(b"1 0 0", b"1e0\t0e-1 0").replace(b"\n", b"\r\n"),
        "reject-empty-input": b"", "reject-incomplete-facet": triangle[:80],
        "reject-missing-vertex": triangle.replace(b"vertex 0 1 0\n", b""),
        "reject-long-line": b"solid " + b"a" * 4097 + b"\nendsolid\n",
        "reject-long-token": triangle.replace(b"vertex 0 0 0", b"vertex " + b"1" * 129 + b" 0 0"),
        "reject-overflow": triangle.replace(b"vertex 0 0 0", b"vertex 1e999 0 0"),
        "reject-nan-vertex": triangle.replace(b"vertex 0 0 0", b"vertex NaN 0 0"),
        "reject-infinite-vertex": triangle.replace(b"vertex 0 0 0", b"vertex inf 0 0"),
        "accept-nan-normal": triangle.replace(b"normal 0 0 0", b"normal NaN 0 0"),
    }
    for name, raw in ascii_cases.items():
        for target in ["ascii_stl", "auto_stl"]:
            # Prefix names to retain both ASCII and binary versions of empty-input cases.
            prefix, suffix = name.split("-", 1)
            emit(target, f"{prefix}-ascii-{suffix}", raw, "synthetic ASCII grammar/numeric case")
    emit("auto_stl", "reject-ambiguous-solid", b"solid\x00" + b" " * 80, "ambiguous malformed header")

    points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, 0, 1)]
    tetra = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
    topology_cases = {
        "empty": ([], []), "isolated": (points, []), "triangle": (points[:3], [[0, 1, 2]]),
        "tetrahedron": (points, tetra), "repeated-index": (points, [[0, 0, 1]]),
        "duplicate-face": (points, [[0, 1, 2], [0, 1, 2]]),
        "opposite-duplicate": (points, [[0, 1, 2], [2, 1, 0]]),
        "three-face-edge": (points + [(0, -1, 0)], [[0, 1, 2], [1, 0, 3], [0, 1, 4]]),
        "conflicting-winding": (points, [[0, 1, 2], [0, 1, 3]]),
        "bow-tie": (points + [(-1, 0, 0)], [[0, 1, 2], [0, 3, 4]]),
        "disconnected": (points + [(x + 3, y, z) for x, y, z in points],
                         tetra + [[v + 4 for v in face] for face in tetra]),
        "vertex-contact": (points + [(-1, 0, 0), (0, -1, 0), (0, 0, -1)],
                           tetra + [[0, 5, 4], [0, 4, 6], [0, 6, 5], [4, 5, 6]]),
        "collinear": ([(0, 0, 0), (1, 0, 0), (2, 0, 0)], [[0, 1, 2]]),
        "high-incidence": ([(0, 0, 0), (1, 0, 0)] + [(0, 1, i / 16) for i in range(128)],
                           [[0, 1, i + 2] for i in range(128)]),
    }
    for name, (vertices, faces) in topology_cases.items():
        emit("topology", name, topology_bytes(vertices, faces), "synthetic bounded indexed mesh")
    emit("topology", "maximum-degenerate-counts", struct.pack("<HH", 512, 1024),
         "short input exercises zero padding and maximum V/F caps")

    report = {"head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "fixture_manifest_sha256": manifest_hash, "verified_fixtures": fixtures, "seeds": records,
              "counts": {target: sum(r["target"] == target for r in records) for target in TARGETS}}
    (args.output / "seed-manifest.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"output": str(args.output), "verified_fixtures": len(fixtures), "seeds": report["counts"]}))


if __name__ == "__main__":
    main()
