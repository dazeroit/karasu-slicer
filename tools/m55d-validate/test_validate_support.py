"""Reader self-checks; do not require Rust or external artifacts."""
from copy import deepcopy
from fractions import Fraction as F
import unittest
from validate_support import expected_pixel, validate_mesh


def cube():
    return {'vertices': [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
                         [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
            'triangles': [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7],
                          [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5],
                          [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]]}


class ReaderTests(unittest.TestCase):
    def test_exact_closed_cube_volume(self):
        self.assertEqual(validate_mesh(cube()), 1)

    def test_missing_face_rejected(self):
        mesh = cube()
        mesh['triangles'].pop()
        with self.assertRaises(ValueError):
            validate_mesh(mesh)

    def test_inward_face_rejected(self):
        mesh = cube()
        mesh['triangles'][0].reverse()
        with self.assertRaises(ValueError):
            validate_mesh(mesh)

    def test_duplicate_or_disconnected_vertex_rejected(self):
        for extra in [[0, 0, 0], [F(1, 2), F(1, 2), F(1, 2)]]:
            mesh = deepcopy(cube())
            mesh['vertices'].append(extra)
            with self.assertRaises(ValueError):
                validate_mesh(mesh)

    def test_exact_sample_boundary_is_half_open(self):
        self.assertEqual(expected_pixel(107, 80, F(5)), 255)
        self.assertEqual(expected_pixel(132, 80, F(5)), 0)
        self.assertEqual(expected_pixel(120, 67, F(5)), 255)
        self.assertEqual(expected_pixel(120, 92, F(5)), 0)

    def test_material_cavity_and_support_are_distinct(self):
        self.assertEqual(expected_pixel(120, 80, F(1, 2)), 255)
        self.assertEqual(expected_pixel(20, 20, F(1, 2)), 0)
        self.assertEqual(expected_pixel(120, 80, F(11)), 255)
        self.assertEqual(expected_pixel(120, 80, F(13)), 0)
        self.assertEqual(expected_pixel(10, 80, F(13)), 255)


if __name__ == '__main__':
    unittest.main()
