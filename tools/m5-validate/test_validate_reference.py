"""Independent reference-reader fixtures built using public PNG/JSON primitives."""
import json
from pathlib import Path
import struct
import tempfile
import unittest
import zlib

from validate_reference import validate


def fixture(directory, pixels=bytes([0, 255]), minimum_gray=None):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    image = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 1, 8, 0, 0, 0, 0))
             + chunk(b"IDAT", zlib.compress(b"\0" + pixels)) + chunk(b"IEND", b""))
    (directory / "layer-00000000.png").write_bytes(image)
    settings = {"pwm": 255, "exposure_s": 2, "lift": [{"distance_mm": 1}], "retract": [{"distance_mm": 1}]}
    record = {"layer": {"index": 0, "thickness_mm": .1, "position_z_mm": .1,
                        "sample_z_mm": .05, "settings": settings},
              "image": "layer-00000000.png", "mask_crc32": zlib.crc32(pixels),
              "statistics": {"intensity_sum": sum(pixels), "nonzero_pixels": sum(v != 0 for v in pixels)},
              "raster": {"samples_per_axis": 1, "grayscale_levels": 256, "outside": "reject",
                         "compensation": {"kind": "none"}}}
    if minimum_gray is not None:
        record["raster"].update(samples_per_axis=4, minimum_gray=minimum_gray)
    line = json.dumps(record) + "\n"
    (directory / "layers.jsonl").write_text(line)
    manifest = {"schema_version": 1, "application": {"name": "karasu-slicer"}, "layers": "layers.jsonl",
                "printer": {"schema_version": 1, "resolution_px": [2, 1], "build_size_mm": [2, 1, 10]},
                "resin": {"schema_version": 1}, "overrides": [], "layer_count": 1,
                "summary": {"layers": 1, "payload_bytes_excluding_manifest": len(image) + len(line),
                            "coverage_volume_mm3": sum(pixels) / 255 * .1}}
    (directory / "manifest.json").write_text(json.dumps(manifest))
    return record, manifest


class ReferenceValidation(unittest.TestCase):
    def test_public_primitive_fixture(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            fixture(root)
            self.assertEqual(validate(root)["source_coverage_volume_mm3"], .1)

    def test_mask_crc_and_truncation_are_rejected(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            record, _ = fixture(root)
            record["mask_crc32"] ^= 1
            (root / "layers.jsonl").write_text(json.dumps(record) + "\n")
            with self.assertRaises(ValueError):
                validate(root)
            fixture(root)
            image = root / "layer-00000000.png"
            image.write_bytes(image.read_bytes()[:-1])
            with self.assertRaises(ValueError):
                validate(root)

    def test_invalid_sample_and_summary_counts_are_rejected(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            record, manifest = fixture(root)
            record["layer"]["sample_z_mm"] = 1
            (root / "layers.jsonl").write_text(json.dumps(record) + "\n")
            with self.assertRaises(ValueError):
                validate(root)
            fixture(root)
            manifest["summary"]["layers"] = 2
            (root / "manifest.json").write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                validate(root)

    def test_minimum_gray_bounds_nonzero_pixels(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            fixture(root, bytes([0, 100]), 100)
            self.assertAlmostEqual(validate(root)["source_coverage_volume_mm3"], 100 / 255 * .1)
            for pixels, minimum in [(bytes([0, 99]), 100), (bytes([0, 255]), 0), (bytes([0, 255]), 256)]:
                with self.subTest(pixels=pixels, minimum=minimum):
                    fixture(root, pixels, minimum)
                    with self.assertRaises(ValueError):
                        validate(root)

    def test_extra_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            fixture(root)
            (root / "unexpected").write_text("unexpected")
            with self.assertRaises(ValueError):
                validate(root)
