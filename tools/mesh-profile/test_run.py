"""Small independent oracles for profiling orchestration; no timing thresholds."""

import math
from pathlib import Path
import tempfile
import unittest

import run


class ProfilingTests(unittest.TestCase):
    def test_statistics_keep_extreme_samples(self):
        result = run.summary([1, 2, 3, 4, 5, 6, 7, 8, 9, 100])
        self.assertEqual(result, dict(median=5.5, minimum=1, maximum=100, iqr=4.5))

    def test_identity_translation_and_scale(self):
        vertices = [(1., 2., 3.), (-4., -5., -6.)]
        self.assertEqual(run.scalar_bounds(vertices, "bounds-identity"), [-4., -5., -6., 1., 2., 3.])
        self.assertEqual(run.scalar_bounds(vertices, "bounds-translation"), [11., -12., -1., 16., -5., 8.])
        for actual, expected in zip(run.scalar_bounds(vertices, "bounds-scale"), [-5., -4., -6.6, 1.25, 1.6, 3.3]):
            self.assertAlmostEqual(actual, expected, places=14)

    def test_combined_pivot_is_fixed_before_translation(self):
        result = run.scalar_bounds([(2., -3., 4.)], "bounds-combined")
        self.assertEqual(result, [17., -10., 9.] * 2)

    def test_rotation_of_basis_point(self):
        result = run.scalar_bounds([(1., 0., 0.)], "bounds-rotation")
        expected = [math.cos(.29) * math.cos(-.47), math.sin(.29) * math.cos(-.47), -math.sin(-.47)]
        for actual, target in zip(result, expected * 2):
            self.assertAlmostEqual(actual, target, places=15)

    def test_session_preserves_existing_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "existing").write_text("preserve", encoding="utf-8")
            with self.assertRaises(ValueError):
                run.Session(path)
            self.assertEqual((path / "existing").read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
