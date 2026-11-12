"""Reader acceptance/rejection tests with separately generated analytic masks."""
from fractions import Fraction
import json
from pathlib import Path
import tempfile
import unittest
from validate_masks import HEADER, PLANES, expected_pixel, read_pgm, validate


class MaskReaderTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.report = {"schema": 1, "fixture": "cube20-bottom-cut-v1",
                       "backend": "convex-cardinal-layer-difference-v1",
                       "panel_dimensions": [200, 200], "panel_extent_mm": [20, 20],
                       "samples_per_axis": 1, "grayscale_levels": 2,
                       "compensation": "none", "enclosed_cavities": 0,
                       "radial_error_mm": 0.001, "axial_error_mm": 0,
                       "hollow_offset_error_mm": 0, "plan_record_hex": "0102",
                       "planes": [{"file": name, "z_mm": float(z)} for name, z in PLANES.items()]}
        for filename, z in PLANES.items():
            pixels = bytearray()
            for y in range(200):
                for x in range(200):
                    # Independent squared-distance oracle for ideal circle and exact square cavity.
                    px, py = 2*x+1, 2*y+1
                    cavity = 2 < z < 18 and 40 < px < 360 and 40 < py < 360
                    circle = z == 1 and (px-200)**2 + (py-200)**2 < 400
                    pixels.append(0 if cavity or circle else 255)
            (self.root / filename).write_bytes(HEADER + pixels)
        self.write_report()

    def write_report(self):
        (self.root / "report.json").write_text(json.dumps(self.report))

    def test_analytic_masks_pass(self):
        self.assertEqual(validate(self.root)["status"], "passed")

    def test_full_top_wall_and_inner_cavity_are_distinguished(self):
        self.assertEqual(expected_pixel(100, 100, Fraction(19), Fraction(1, 1000)), 255)
        self.assertEqual(expected_pixel(100, 100, Fraction(3), Fraction(1, 1000)), 0)

    def test_circle_center_and_far_material(self):
        self.assertEqual(expected_pixel(100, 100, Fraction(1), Fraction(1, 1000)), 0)
        self.assertEqual(expected_pixel(20, 20, Fraction(1), Fraction(1, 1000)), 255)

    def test_missing_opening_rejected(self):
        path = self.root / "z1.pgm"
        path.write_bytes(HEADER + bytes([255])*40000)
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_wrong_cap_mask_rejected(self):
        path = self.root / "z3.pgm"
        path.write_bytes(HEADER + bytes([255])*40000)
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_nonbinary_and_extra_payload_rejected(self):
        path = self.root / "bad.pgm"
        for data in [HEADER + bytes([1])*40000, HEADER + bytes(40001), b"P5\n200 200\n1\n" + bytes(40000)]:
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                read_pgm(path)

    def test_error_outside_requested_budget_rejected(self):
        self.report["radial_error_mm"] = 0.002
        self.write_report()
        with self.assertRaises(ValueError):
            validate(self.root)

    def test_duplicate_or_relabelled_planes_rejected(self):
        self.report["planes"][0]["z_mm"] = 3
        self.write_report()
        with self.assertRaises(ValueError):
            validate(self.root)


if __name__ == "__main__":
    unittest.main()
