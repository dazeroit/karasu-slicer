"""Independent-reader oracle regressions; no Rust toolchain required."""
from fractions import Fraction as F
import unittest
from validate_preparation import expected_pixel, check_mask


class ReaderTests(unittest.TestCase):
    def test_support_and_platform_caps(self):
        self.assertEqual(expected_pixel(120, 80, F(0)), 255)
        self.assertEqual(expected_pixel(120, 80, F(-1)), 0)
        self.assertEqual(expected_pixel(10, 10, F(5)), 0)

    def test_bottom_wall_has_draining_opening(self):
        self.assertEqual(expected_pixel(50, 50, F(10)), 0)
        self.assertEqual(expected_pixel(50, 50, F(11)), 0)
        self.assertEqual(expected_pixel(120, 80, F(11)), 255)

    def test_cavity_bottom_cap_is_subtracted(self):
        self.assertEqual(expected_pixel(120, 80, F(12)), 0)
        self.assertEqual(expected_pixel(10, 10, F(12)), 255)
        self.assertEqual(expected_pixel(120, 80, F(13)), 0)

    def test_taper_uses_half_open_sample_boundary(self):
        self.assertEqual(expected_pixel(107, 80, F(5)), 255)
        self.assertEqual(expected_pixel(132, 80, F(5)), 0)

    def test_only_certified_radial_band_is_indeterminate(self):
        self.assertIsNone(expected_pixel(59, 50, F(11), F(1, 10)))
        self.assertEqual(expected_pixel(59, 50, F(11), F(1, 1000)), 0)
        self.assertEqual(expected_pixel(70, 50, F(11), F(1, 1000)), 255)

    def test_nonbinary_or_corrupt_mask_rejected(self):
        for pixels in [b'', bytes([3])*40000, bytes([255])*40000]:
            with self.assertRaises(ValueError):
                check_mask(pixels, F(13), F(1, 1000))


if __name__ == '__main__':
    unittest.main()
