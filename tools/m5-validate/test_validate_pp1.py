"""Malformed fixtures and independent PW0/component decoding; no Rust helpers imported."""
import unittest

from validate_pp1 import decode_pw0, raster_components


class Pp1Validation(unittest.TestCase):
    def test_literal_public_protocol_runs(self):
        self.assertEqual(decode_pw0(bytes.fromhex("0002f0032421"), 10), bytes([0, 0, 255, 255, 255, 34, 34, 34, 34, 34]))

    def test_zero_run_is_rejected(self):
        with self.assertRaises(ValueError):
            decode_pw0(bytes.fromhex("100001"), 1)

    def test_truncated_long_run_is_rejected(self):
        with self.assertRaises(ValueError):
            decode_pw0(b"\xf0", 1)

    def test_overrun_and_underrun_are_rejected(self):
        for data, count in [(bytes.fromhex("f002"), 1), (bytes.fromhex("f001"), 2)]:
            with self.assertRaises(ValueError):
                decode_pw0(data, count)

    def test_eight_connected_diagonal_and_separate_components(self):
        self.assertEqual(raster_components(bytes([255, 0, 0, 0, 255, 0, 0, 0, 255]), 3, 3), (1, 3))
        self.assertEqual(raster_components(bytes([255, 0, 255, 0, 0, 0]), 3, 2), (2, 1))


if __name__ == "__main__":
    unittest.main()
