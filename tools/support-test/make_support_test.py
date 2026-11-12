#!/usr/bin/env python3
"""Write the support test plate (docs/research/SUPPORT-PRESETS-RESEARCH.md) as a karasu project.

Eighteen tiles, 15 x 15 x 2 mm, lifted 8 mm on supports over a raft, each numbered by digits
embossed on its top (0.6 mm tall):

- Grid A, tiles 1 to 12: the contact. Diameter 0.3, 0.4, 0.5 and 0.6 mm (columns) against
  depth 0.1, 0.2 and 0.3 mm (rows), on the Medium preset otherwise; 36 supports 2.5 mm apart,
  every one to the raft, no brace.
- Grid B, tiles 13 to 18: the structure. Medium's contact (0.4 mm sunk 0.2 mm) with braces
  (13), 3.5 mm apart (14), 3.5 mm apart with braces (15), branching from trunks (16); Heavy as
  the venom torso v4 had it, branching and braced (17), and Heavy alone (18).

The printer, resin, print setting and plate settings are copied from a template project (the
owner's), so the plate prints as the owner's prints do. Every support is written in the
tile's own frame (the tile's underside is z = 0); brace and branch heights are world heights.

The second plate (`--round 2`, after the first's print, E-SUP-02) confirms what the first
showed, two tiles of each of ten combinations (tiles 1 to 10, then 11 to 20 in the same order,
set elsewhere on the plate): the contact's width at the surface about the threshold (0.45,
0.48 and 0.52 mm), depth 0.1 against 0.2 mm, Heavy's contact on a thin and on its own body,
16 wide contacts 3.5 mm apart, sparse braces, and tiles tilted by 30 degrees. Every tile of
the second plate is held to its edges and corners (the first plate's curled there), a tilted
one from its lowest edge.

Usage: make_support_test.py [--round 2] <template.karasu> <output.karasu> [<sheet.md>]
"""
from __future__ import annotations

import hashlib
import io
import json
import struct
import sys
import zipfile

TILE = 15.0
THICK = 2.0
LIFT = 8.0
PITCH = 26.0
DIGIT_H = 0.6
RAFT = {
    "fillet_diameter_mm": 2.0,
    "fillet_height_mm": 0.8,
    "margin_mm": 1.0,
    "slope_deg": 60.0,
    "thickness_mm": 0.5,
}
# Brace ends, in world mm: above the raft's fillets, below the tips' joins (8 - 3).
BRACE_LOW, BRACE_HIGH = 2.0, 4.5
# Where a branch joins its trunk, in world mm.
BRANCH_Z = 3.0

# Seven-segment digits: segments a to g.
SEGMENTS = {
    "0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc",
    "5": "afgcd", "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcdfg",
}


def box(x0, y0, z0, x1, y1, z1):
    """The 12 outward triangles of an axis-aligned box."""
    v = [(x, y, z) for z in (z0, z1) for y in (y0, y1) for x in (x0, x1)]
    quads = [(0, 2, 3, 1), (4, 5, 7, 6), (0, 1, 5, 4), (2, 6, 7, 3), (0, 4, 6, 2), (1, 3, 7, 5)]
    tris = []
    for a, b, c, d in quads:
        tris += [(v[a], v[b], v[c]), (v[a], v[c], v[d])]
    return tris


def digit_boxes(ch, x, y, w=4.0, h=7.0, t=0.8, gap=0.1):
    """The segments of digit `ch` with its lower left corner at (x, y), as boxes on the top,
    `gap` apart so that no two share an edge (each stays a closed, manifold box)."""
    z0, z1 = THICK, THICK + DIGIT_H
    mid = y + h / 2
    seg = {
        "a": (x + t + gap, y + h - t, x + w - t - gap, y + h),
        "d": (x + t + gap, y, x + w - t - gap, y + t),
        "g": (x + t + gap, mid - t / 2, x + w - t - gap, mid + t / 2),
        "f": (x, mid + gap, x + t, y + h),
        "b": (x + w - t, mid + gap, x + w, y + h),
        "e": (x, y, x + t, mid - gap),
        "c": (x + w - t, y, x + w, mid - gap),
    }
    out = []
    for s in SEGMENTS[ch]:
        a0, b0, a1, b1 = seg[s]
        out += box(a0, b0, z0, a1, b1, z1)
    return out


def tile_stl(number: int) -> bytes:
    """A tile with its number embossed: the tile and each segment are closed boxes, the
    segments standing on the tile's top."""
    tris = box(0, 0, 0, TILE, TILE, THICK)
    text = str(number)
    width = len(text) * 4.0 + (len(text) - 1) * 1.5
    x = (TILE - width) / 2
    for ch in text:
        tris += digit_boxes(ch, x, (TILE - 7.0) / 2)
        x += 5.5
    out = io.BytesIO()
    out.write(b"karasu support test tile".ljust(80, b" "))
    out.write(struct.pack("<I", len(tris)))
    for a, b, c in tris:
        u = [b[k] - a[k] for k in range(3)]
        w = [c[k] - a[k] for k in range(3)]
        n = (u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0])
        out.write(struct.pack("<3f", *n))
        for p in (a, b, c):
            out.write(struct.pack("<3f", *p))
        out.write(b"\0\0")
    return out.getvalue()


def grid(spacing: float, inset=None) -> list[tuple[float, float]]:
    """Points `spacing` apart, centred on the tile's underside; with `inset` (low and high X,
    then low and high Y, in mm from the edges), as many points as `spacing` gives, spread from
    edge to edge, the outer rows and the corners that far in."""
    count = int((TILE - 2.5) // spacing) + 1
    if inset is None:
        start = (TILE - (count - 1) * spacing) / 2
        x = y = [start + i * spacing for i in range(count)]
    else:
        x_lo, x_hi, y_lo, y_hi = inset
        x = [x_lo + i * (TILE - x_lo - x_hi) / (count - 1) for i in range(count)]
        y = [y_lo + j * (TILE - y_lo - y_hi) / (count - 1) for j in range(count)]
    return [(x[i], y[j]) for j in range(count) for i in range(count)]


# On the second plate the outer tips stand this far in from the edges, in mm, so the edges and
# corners are held (the first plate's tiles curled where no tip reached: E-SUP-02).
EDGE = 0.4
# How far in from the lowest edge of a tilted tile its tips stand, in mm: that edge prints
# first, and must stand on tips from its first layer (owner, 2026-11-06).
LOW_EDGE = 0.2


def document(preset, defaults, spacing, braces, branches, inset=None):
    """A support document for one tile: every point of the grid to the raft, or the points
    branching from trunks (every other point in both directions), with braces between
    neighbouring standing supports (`braces` true), or between every other pair along the rows
    only ("sparse")."""
    points = grid(spacing, inset)
    count = int(round(len(points) ** 0.5))
    supports, ids, trunks = [], {}, {}
    next_id = 1
    for k, (x, y) in enumerate(points):
        i, j = k % count, k // count
        trunk = not branches or (i % 2 == 0 and j % 2 == 0)
        if trunk:
            supports.append({
                "id": next_id, "kind": "standard", "origin": "placed",
                "tip": {"axis": "normal", "point": [x, y, 0.0]},
                "bends": [], "base": {"vertical": True}, "accept_collision": False,
            })
            ids[(i, j)] = next_id
            trunks[(i, j)] = next_id
            next_id += 1
    if branches:
        for k, (x, y) in enumerate(points):
            i, j = k % count, k // count
            if (i, j) in trunks:
                continue
            parent = trunks[(i - i % 2, j - j % 2)]
            supports.append({
                "id": next_id, "kind": "standard", "origin": "placed",
                "tip": {"axis": "straight", "point": [x, y, 0.0]},
                "bends": [], "base": {"support": parent, "z_mm": BRANCH_Z},
                "accept_collision": False,
            })
            next_id += 1
    brace_list = []
    if braces:
        standing = sorted(trunks.items())
        step = 2 if branches else 1
        for (i, j), sid in standing:
            pairs = ((step, 0), (0, step)) if braces is True else (((1, 0),) if i % 2 == 0 else ())
            for di, dj in pairs:
                other = trunks.get((i + di, j + dj))
                if other is not None:
                    brace_list.append({
                        "id": next_id,
                        "from": {"support": sid, "z_mm": BRACE_LOW},
                        "to": {"support": other, "z_mm": BRACE_HIGH},
                    })
                    next_id += 1
    return {
        "format": "karasu-supports", "version": 1, "preset": preset, "defaults": defaults,
        "clearance_mm": 0.3, "anchor_tolerance_mm": 0.05, "max_axis_deviation_deg": 60.0,
        "raft": dict(RAFT), "supports": supports, "braces": brace_list,
    }


def tiles():
    """Each tile of the first plate: its number, its place (column, row), its description, its
    support document and its tilt about X in degrees."""
    out = []
    for r, depth in enumerate((0.1, 0.2, 0.3)):
        for c, diameter in enumerate((0.3, 0.4, 0.5, 0.6)):
            number = r * 4 + c + 1
            defaults = {"contact_diameter_mm": diameter, "penetration_mm": depth}
            out.append((number, (c, r), f"A: contact {diameter} mm, depth {depth} mm",
                        document("medium", defaults, 2.5, False, False), 0.0))
    medium = {"contact_diameter_mm": 0.4, "penetration_mm": 0.2}
    grid_b = [
        (13, "B: Medium, 2.5 mm, braced", "medium", medium, 2.5, True, False),
        (14, "B: Medium, 3.5 mm apart", "medium", medium, 3.5, False, False),
        (15, "B: Medium, 3.5 mm apart, braced", "medium", medium, 3.5, True, False),
        (16, "B: Medium, 2.5 mm, branching from trunks", "medium", medium, 2.5, False, True),
        (17, "B: Heavy (0.6 mm sunk 0.3 mm), branching and braced, as the torso v4", "heavy", {},
         2.5, True, True),
        (18, "B: Heavy (0.6 mm sunk 0.3 mm) alone", "heavy", {}, 2.5, False, False),
    ]
    for k, (number, text, preset, defaults, spacing, braced, branched) in enumerate(grid_b):
        out.append((number, (4 + k // 3, k % 3), text,
                    document(preset, defaults, spacing, braced, branched), 0.0))
    return out


def surface(d, depth, body=1.0, length=2.5):
    """A cone contact's width where it meets the surface (it ends `depth` inside)."""
    return d + (body - d) * depth / length


def tiles_round_2():
    """Each tile of the second plate, as `tiles` gives them: ten combinations, twice."""
    def medium(d, depth, **more):
        return {"contact_diameter_mm": d, "penetration_mm": depth, **more}

    thin_heavy = {"penetration_mm": 0.1, "tip_body_diameter_mm": 1.0, "shaft_diameter_mm": 1.0,
                  "tip_length_mm": 2.5}
    combos = [
        ("threshold", "medium", medium(0.43, 0.1), 2.5, False, 0.0),
        ("threshold", "medium", medium(0.46, 0.1), 2.5, False, 0.0),
        ("reference, as tile 3 of the first plate", "medium", medium(0.5, 0.1), 2.5, False, 0.0),
        ("depth, as tile 7 of the first plate", "medium", medium(0.5, 0.2), 2.5, False, 0.0),
        ("Heavy's contact on a thin body (tip and shaft 1.0 mm, tip 2.5 mm long)", "heavy",
         thin_heavy, 2.5, False, 0.0),
        ("Heavy's contact on its own body (tip and shaft 1.4 mm, tip 3.0 mm long)", "heavy",
         {"penetration_mm": 0.1}, 2.5, False, 0.0),
        # A 3.5 mm spacing gives four rows, which the edge insets spread 4.7 mm apart.
        ("16 tips 4.7 mm apart: the contacts' total area (2.1 %) below the threshold with a wide "
         "contact", "medium", medium(0.6, 0.1), 3.5, False, 0.0),
        ("sparse braces (every other pair along the rows)", "medium", medium(0.5, 0.1), 2.5,
         "sparse", 0.0),
        ("tilted by 30 degrees", "medium", medium(0.5, 0.1), 2.5, False, 30.0),
        ("today's Medium, tilted by 30 degrees", "medium", medium(0.4, 0.2), 2.5, False, 30.0),
    ]
    out = []
    columns = 7
    for copy in range(2):
        for k, (what, preset, defaults, spacing, braces, tilt) in enumerate(combos):
            number = copy * len(combos) + k + 1
            # The second copy fills the slots from the far end, so that the two copies of a
            # combination print in different places.
            slot = k if copy == 0 else 2 * len(combos) - 1 - k
            body = defaults.get("tip_body_diameter_mm", 1.4 if preset == "heavy" else 1.0)
            length = defaults.get("tip_length_mm", 3.0 if preset == "heavy" else 2.5)
            d = defaults.get("contact_diameter_mm", 0.6)
            width = surface(d, defaults["penetration_mm"], body, length)
            text = (f"{k + 1}: contact {d} mm, depth {defaults['penetration_mm']} mm "
                    f"(surface {width:.2f} mm), {what}")
            # Every tile is held to its edges and corners; a tilted one closer at its lowest edge.
            edges = (EDGE, EDGE, LOW_EDGE if tilt else EDGE, EDGE)
            if tilt:
                text += f", its lowest edge's tips {LOW_EDGE} mm in"
            out.append((number, (slot % columns, slot // columns), text,
                        document(preset, defaults, spacing, braces, False, edges), tilt))
    return out


def main(argv):
    second = len(argv) > 1 and argv[1:3] == ["--round", "2"]
    if second:
        argv = argv[:1] + argv[3:]
    if len(argv) not in (3, 4):
        sys.exit(__doc__)
    with zipfile.ZipFile(argv[1]) as template:
        source = json.loads(template.read("karasu-project.json"))
    plate = source["plates"][0]
    printer = plate["printer"]
    [ox, oy] = printer["origin_xy_mm"]
    [width, depth, _] = printer["build_size_mm"]
    made = tiles_round_2() if second else tiles()
    columns, rows = (7, 3) if second else (6, 3)
    x0 = ox + (width - (columns - 1) * PITCH - TILE) / 2
    y0 = oy + (depth - (rows - 1) * PITCH - TILE) / 2
    models, objects, files, sheet = [], [], {}, []
    for number, (c, r), text, doc, tilt in made:
        stl = tile_stl(number)
        sha = hashlib.sha256(stl).hexdigest()
        name = f"tile_{number:02d}.stl"
        models.append({"sha256": sha, "name": name})
        files[f"models/{sha}.stl"] = stl
        objects.append({
            "name": f"{number:02d} {text}",
            "model": sha,
            "units": "millimeter",
            # The tile's front row is row 0, nearest the plate's front.
            "translate_mm": [x0 + c * PITCH, y0 + (rows - 1 - r) * PITCH, 0.0],
            "pose": {"rotate_deg": [tilt, 0.0, 0.0], "scale": [1.0, 1.0, 1.0],
                     "mirror": [False, False, False], "center": False, "drop": True,
                     "lift_mm": LIFT},
            "operations": [],
            "supports": doc,
            "accept_support_collisions": False,
            "copies": 1,
            "arrange": False,
            "protected": [],
        })
        sheet.append((number, text, len(doc["supports"]), len(doc["braces"])))
    project = {
        "format": "karasu-project", "version": 1,
        "name": "support_test_2" if second else "support_test",
        "next_plate": 2, "models": models,
        "plates": [{
            "id": 1, "name": "Support test 2" if second else "Support test", "printer": printer, "resin": plate["resin"],
            "print_setting": plate["print_setting"], "settings": plate["settings"],
            "objects": objects,
        }],
    }
    with zipfile.ZipFile(argv[2], "w", zipfile.ZIP_STORED) as out:
        out.writestr("karasu-project.json", json.dumps(project, indent=1))
        for path, data in files.items():
            out.writestr(path, data)
    if len(argv) == 4:
        lines = ["| Tile | What it tests | Supports | Braces | Held? | Removal (1-5) | Marks |",
                 "| --- | --- | --- | --- | --- | --- | --- |"]
        lines += [f"| {n} | {t} | {s} | {b} | | | |" for n, t, s, b in sorted(sheet)]
        with open(argv[3], "w", encoding="utf-8") as out:
            out.write("\n".join(lines) + "\n")
    print(f"{len(objects)} tiles, {sum(s for *_, s, _ in sheet)} supports, "
          f"{sum(b for *_, b in sheet)} braces -> {argv[2]}")


if __name__ == "__main__":
    main(sys.argv)
