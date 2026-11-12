#!/usr/bin/env python3
"""Independent exact-center oracle for drain_reference's four binary masks.

Only the published radial-error annulus is indeterminate. The reader checks the
known cube/cavity fixture, not an arbitrary geometry or a physical printer file.
"""
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path

HEADER = b"P5\n200 200\n255\n"
PLANES = {"z1.pgm": Fraction(1), "z2_5.pgm": Fraction(5, 2),
          "z3.pgm": Fraction(3), "z19.pgm": Fraction(19)}


def read_pgm(path):
    data = Path(path).read_bytes()
    if not data.startswith(HEADER) or len(data) != len(HEADER) + 40000:
        raise ValueError(f"noncanonical PGM header or payload size: {path}")
    pixels = data[len(HEADER):]
    if any(pixel not in (0, 255) for pixel in pixels):
        raise ValueError(f"nonbinary pixel: {path}")
    return pixels


def expected_pixel(x, y, z, radial_error):
    """Return 0/255, or None only inside the certified circle-error annulus."""
    # Exact pixel-center positions of a 20 mm / 200 pixel panel.
    px, py = Fraction(2*x+1, 20), Fraction(2*y+1, 20)
    cavity = 2 < z < 18 and 2 < px < 18 and 2 < py < 18
    if cavity:
        return 0
    if z == 1:
        radius_squared = (px-10)**2 + (py-10)**2
        if radius_squared < (1-radial_error)**2:
            return 0
        if radius_squared <= (1+radial_error)**2:
            return None
    return 255


def validate(directory):
    root = Path(directory)
    report = json.loads((root / "report.json").read_text(), parse_float=Fraction)
    if (report["schema"] != 1 or report["fixture"] != "cube20-bottom-cut-v1"
            or report["backend"] != "convex-cardinal-layer-difference-v1"
            or report["panel_dimensions"] != [200, 200]
            or report["panel_extent_mm"] != [20, 20]
            or report["samples_per_axis"] != 1 or report["grayscale_levels"] != 2
            or report["compensation"] != "none" or report["enclosed_cavities"] != 0):
        raise ValueError("unsupported fixture/report configuration")
    for key in ("radial_error_mm", "axial_error_mm", "hollow_offset_error_mm"):
        if not 0 <= report[key] <= Fraction(1, 1000):
            raise ValueError(f"invalid error certificate: {key}")
    record = report["plan_record_hex"]
    if not record or bytes.fromhex(record).hex() != record:
        raise ValueError("noncanonical or empty plan record")
    if len(report["planes"]) != 4 or {p["file"]: p["z_mm"] for p in report["planes"]} != PLANES:
        raise ValueError("unexpected mask planes")
    result = {}
    for filename, z in PLANES.items():
        pixels = read_pgm(root / filename)
        checked = uncertain = 0
        for y in range(200):
            for x in range(200):
                expected = expected_pixel(x, y, z, report["radial_error_mm"])
                if expected is None:
                    uncertain += 1
                else:
                    checked += 1
                    if pixels[y*200+x] != expected:
                        raise ValueError(f"incorrect material pixel: {filename} ({x},{y})")
        result[filename] = {"checked_pixels": checked, "radial_band_pixels": uncertain,
                            "material_pixels": pixels.count(255),
                            "sha256": hashlib.sha256((root / filename).read_bytes()).hexdigest()}
    return {"schema": 1, "status": "passed", "masks": result,
            "plan_record_sha256": hashlib.sha256(bytes.fromhex(record)).hexdigest()}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    arguments = parser.parse_args()
    print(json.dumps(validate(arguments.directory), indent=2, sort_keys=True))
