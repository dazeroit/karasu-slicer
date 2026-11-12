"""Independent positive and corruption regressions for the hollow geometry reader."""
import copy
from fractions import Fraction
import json
from pathlib import Path
import tempfile
import unittest

import validate_hollow as reader


def cube(low, high):
    points = [[low, low, low], [high, low, low], [high, high, low], [low, high, low],
              [low, low, high], [high, low, high], [high, high, high], [low, high, high]]
    faces = [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4],
             [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]]
    return {"vertices": points, "triangles": faces}


def fixture():
    source, cavity = cube(0, 20), cube(2, 18)
    shell = {"vertices": source["vertices"]+cavity["vertices"],
             "triangles": source["triangles"]+[[a+8, c+8, b+8] for a, b, c in cavity["triangles"]]}
    geometry = {"schema": 1, "source": source, "cavity": cavity, "shell": shell}
    report = {"backend": "convex-halfspaces-v1", "wall_offset_error_mm": 0,
              "vertices": 16, "triangles": 24, "source_volume_mm3": [8000, 8000], "cavity_volume_mm3": [4096, 4096],
              "material_volume_mm3": [3904, 3904]}
    return geometry, report


class HollowValidationTests(unittest.TestCase):
    def test_exact_cube_and_rational_volume(self):
        geometry, report = fixture()
        self.assertTrue(reader.validate_documents(geometry, report, cube=True)["validated"])
        self.assertEqual(reader.volume(*reader.mesh(geometry["cavity"])), Fraction(4096))

    def test_report_bounds_and_missing_keys_are_rejected(self):
        geometry, report = fixture()
        for interval in ([4097, 4098], [4097, 4095], [-1, 5000], [4096], [float("nan"), 4096]):
            with self.subTest(interval=interval):
                altered = copy.deepcopy(report)
                altered["cavity_volume_mm3"] = interval
                with self.assertRaises(reader.ValidationError):
                    reader.validate_documents(geometry, altered)
        del report["source_volume_mm3"]
        with self.assertRaises(KeyError):
            reader.validate_documents(geometry, report)

    def test_changed_triangle_winding_is_rejected(self):
        geometry, report = fixture()
        geometry["cavity"]["triangles"][0].reverse()
        with self.assertRaises(reader.ValidationError):
            reader.validate_documents(geometry, report)

    def test_unshared_vertex_and_duplicate_coordinates_are_rejected(self):
        geometry, report = fixture()
        geometry["source"]["vertices"].append([0, 0, 0])
        with self.assertRaises(reader.ValidationError):
            reader.validate_documents(geometry, report)

    def test_certified_offset_cannot_exceed_actual_wall(self):
        geometry, report = fixture()
        with self.assertRaisesRegex(reader.ValidationError, "inward offset"):
            reader.validate_documents(geometry, report, thickness=3)

    def test_shell_face_order_and_reversal_are_verified(self):
        geometry, report = fixture()
        geometry["shell"]["triangles"] = geometry["shell"]["triangles"][::-1]
        with self.assertRaisesRegex(reader.ValidationError, "face preservation"):
            reader.validate_documents(geometry, report)

    def test_nonconvex_source_is_rejected(self):
        geometry, _ = fixture()
        geometry["source"]["vertices"][6] = [8, 8, 8]
        with self.assertRaisesRegex(reader.ValidationError, "supporting plane"):
            reader.convex_supports(*reader.mesh(geometry["source"]))

    def test_json_truncation_duplicate_keys_and_nonfinite_constants_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)/"invalid.json"
            for data in ('{"schema":', '{"schema":1,"schema":1}', '{"value":NaN}'):
                path.write_text(data)
                with self.subTest(data=data), self.assertRaises(ValueError):
                    reader.load_document(path)
            path.write_text(json.dumps(fixture()[0]))
            document, digest = reader.load_document(path)
            self.assertEqual(document["schema"], 1)
            self.assertEqual(len(digest), 64)


if __name__ == "__main__":
    unittest.main()
