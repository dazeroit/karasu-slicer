"""Reader rejects missing support material and blocked drainage independently of the Rust generator."""
import hashlib
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import validate_stress


def write_fixture(root):
    digest = hashlib.sha256(Path(validate_stress.__file__).with_name('source.stl').read_bytes()).digest()
    model = b'KPREP1'+digest
    record = b'KSUP1'+len(model).to_bytes(8, 'little')+model
    report = {'fixture': 'eight-separated-supports-v1', 'support_count': 8, 'component_count': 24,
              'plan_record': list(record),
              'planes': [{'z_mm': 0.5, 'material_pixels': 3200},
                         {'z_mm': 11.0, 'material_pixels': 39684}]}
    (root/'report.json').write_text(json.dumps(report))
    base = bytearray(40000)
    for x in (9, 12, 15, 18):
        for y in (3, 9):
            for row in range((y-1)*10, (y+1)*10):
                for column in range((x-1)*10, (x+1)*10):
                    base[row*200+column] = 255
    wall = bytearray([255])*40000
    for row in range(200):
        for column in range(200):
            if (2*column+1-100)**2+(2*row+1-100)**2 < 400:
                wall[row*200+column] = 0
    for name, mask in [('z0_5.pgm', base), ('z11.pgm', wall)]:
        (root/name).write_bytes(b'P5\n200 200\n255\n'+mask)


class StressReaderTests(unittest.TestCase):
    def test_owned_analytic_masks_pass(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            write_fixture(root)
            self.assertEqual(validate_stress.validate(root)['status'], 'passed')

    def test_missing_support_pixel_is_rejected(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            write_fixture(root)
            path = root/'z0_5.pgm'
            data = bytearray(path.read_bytes())
            data[len(b'P5\n200 200\n255\n')+30*200+90] = 0
            path.write_bytes(data)
            with self.assertRaisesRegex(ValueError, 'pixel'):
                validate_stress.validate(root)

    def test_blocked_drain_pixel_is_rejected(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            write_fixture(root)
            path = root/'z11.pgm'
            data = bytearray(path.read_bytes())
            data[len(b'P5\n200 200\n255\n')+50*200+50] = 255
            path.write_bytes(data)
            with self.assertRaisesRegex(ValueError, 'pixel'):
                validate_stress.validate(root)

    def test_changed_source_identity_is_rejected(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            write_fixture(root)
            path = root/'report.json'
            report = json.loads(path.read_text())
            report['plan_record'][19] ^= 1
            path.write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, 'source identity'):
                validate_stress.validate(root)

    def test_rejects_incorrect_declared_counts_before_reading_masks(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'report.json').write_text(json.dumps({'fixture': 'eight-separated-supports-v1',
                                                       'support_count': 7, 'component_count': 24}))
            with self.assertRaisesRegex(ValueError, 'counts'):
                validate_stress.validate(root)

    def test_rejects_another_fixture_identity(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'report.json').write_text(json.dumps({'fixture': 'other'}))
            with self.assertRaisesRegex(ValueError, 'fixture'):
                validate_stress.validate(root)


if __name__ == '__main__':
    unittest.main()
