"""Tests of the orientation check's model, readers and verdicts.

Run: python3 -m unittest discover -s tools/orientation-check -p 'test_*.py'
"""
import struct
import tempfile
import unittest
from collections import Counter
from pathlib import Path

import compare
import make_f
import print_files


def f_cells(dx=0, dy=0):
    """The F's rectangles as cells of 1 mm, offset by (dx, dy), with row 0 at the bottom."""
    return {(x + dx, y + dy) for x0, y0, x1, y1 in make_f.RECTS
            for x in range(x0, x1) for y in range(y0, y1)}


class Model(unittest.TestCase):
    def test_the_f_is_a_closed_surface_of_the_expected_volume(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'f.stl'
            count = make_f.write(path)
            data = path.read_bytes()
        self.assertEqual(count, 52)
        edges, volume = Counter(), 0.0
        for i in range(count):
            values = struct.unpack_from('<12f', data, 84 + 50 * i)
            a, b, c = values[3:6], values[6:9], values[9:12]
            for p, q in ((a, b), (b, c), (c, a)):
                edges[(p, q)] += 1
            volume += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                       + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6
        # Every directed edge appears once and its reverse once: closed, consistently wound.
        self.assertTrue(all(n == 1 and edges[(q, p)] == 1 for (p, q), n in edges.items()))
        self.assertAlmostEqual(volume, 324 * make_f.THICKNESS, places=4)

    def test_the_f_has_no_symmetry_of_the_panel(self):
        cells = f_cells()
        for kind in ('mirror X', 'mirror Y', 'turned 180'):
            with self.subTest(kind=kind):
                flipped = compare.centred(compare.flip(cells, kind, 20, 30))
                self.assertLess(compare.iou(compare.centred(cells), flipped), 0.5)


class Verdicts(unittest.TestCase):
    def test_each_flip_of_the_f_is_named(self):
        cols, rows = 200, 120
        reference = f_cells(90, 45)
        for kind in compare.FLIPS:
            with self.subTest(kind=kind):
                candidate = compare.flip(reference, kind, cols, rows)
                self.assertEqual(compare.judge(reference, candidate, cols, rows)['verdict'], kind)

    def test_a_shifted_placement_still_matches_once_centred(self):
        result = compare.judge(f_cells(90, 45), f_cells(97, 41), 200, 120)
        self.assertEqual(result['verdict'], 'as stored')
        self.assertLess(result['in_place']['as stored'], 0.85)

    def test_different_shapes_are_unclear(self):
        square = {(x, y) for x in range(80, 110) for y in range(40, 70)}
        self.assertIsNone(compare.judge(f_cells(90, 45), square, 200, 120)['verdict'])

    def test_a_silhouette_fills_each_row_between_its_ends(self):
        width, height = 8, 2
        pixels = bytes([0, 9, 0, 0, 0, 9, 0, 0,
                        0, 0, 0, 0, 0, 0, 0, 0])
        self.assertEqual(compare.silhouette(pixels, width, height, 1),
                         {(1, 0), (2, 0), (3, 0), (4, 0), (5, 0)})


class Transposition(unittest.TestCase):
    def test_transposing_exchanges_rows_and_columns(self):
        # 3 columns by 2 rows becomes 2 columns by 3 rows.
        self.assertEqual(compare.transpose(bytes([1, 2, 3, 4, 5, 6]), 3, 2),
                         bytes([1, 4, 2, 5, 3, 6]))

    def test_transposed_verdicts_name_the_handedness(self):
        self.assertEqual(compare.describe({'verdict': 'mirror X', 'mode': 'transposed'}),
                         'turned 90 degrees (transposed, mirror X), same handedness')
        self.assertTrue(compare.describe({'verdict': 'as stored', 'mode': 'transposed'})
                        .startswith('MIRRORED'))
        self.assertEqual(compare.describe({'verdict': 'as stored', 'mode': 'exchanged panel'}),
                         'same image on an exchanged panel')

    def test_an_exchanged_panel_compares_centred_only(self):
        result = compare.judge(f_cells(90, 45), f_cells(40, 80), 120, 200, in_place=False)
        self.assertEqual(result['verdict'], 'as stored')
        self.assertEqual(result['in_place']['as stored'], 0.0)


class Decoders(unittest.TestCase):
    def test_pw0_runs_decode_and_a_truncated_end_stays_unlit(self):
        # Level 15 for 3 pixels (12-bit run), level 0 for 2, then a level 5 run of 2.
        data = bytes([0xF0, 3, 0x00, 2, 0x52])
        expected = bytes([255] * 3 + [0, 0] + [85] * 2 + [0, 0])
        self.assertEqual(print_files.decode_pw0(data, 9), expected)

    def test_bit_planes_light_a_pixel_lit_in_any_plane(self):
        # Plane 1: 2 lit, 2 unlit; plane 2: 1 unlit, 3 lit.
        data = bytes([0x82, 0x02, 0x01, 0x83])
        self.assertEqual(print_files.decode_planes(data, 4, 2), b'\xff\xff\xff\xff')


if __name__ == '__main__':
    unittest.main()
