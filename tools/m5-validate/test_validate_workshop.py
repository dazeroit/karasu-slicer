"""Hand-built Photon Workshop fixtures, independent of the Rust writer."""
import struct
import tempfile
import unittest
from pathlib import Path

from validate_workshop import decode_planes, decode_pw0, validate


def name(tag):
    return tag.encode().ljust(12, b'\0')


def build(version=516, image=bytes([0x00, 0x01, 0xF0, 0x01, 0x00, 0x01]), planes=False):
    """A 3 x 1 single-layer file: pixels 0, 255, 0 (PW0) or one bit-plane."""
    mark_bytes, areas = {1: (48, 4), 515: (48, 5), 516: (52, 8), 517: (56, 9), 518: (64, 11)}[version]
    sections = {}
    out = bytearray(mark_bytes)
    out[:8] = b'ANYCUBIC'
    struct.pack_into('<II', out, 12, version, areas)
    header_bytes = {1: 96, 515: 96, 516: 100, 517: 108, 518: 112}[version]
    sections['header'] = len(out)
    header = bytearray(header_bytes)
    header[:12] = name('HEADER')
    struct.pack_into('<I', header, 12, header_bytes - 16)
    struct.pack_into('<III', header, 56, 1, 3, 1)
    out += header
    sections['preview'] = len(out)
    out += name('PREVIEW') + struct.pack('<IIII', 224 * 168 * 2 + 12, 224, 42, 168) + bytes(224 * 168 * 2)
    if version >= 515:
        sections['colours'] = len(out)
        out += struct.pack('<II', 0, 16) + bytes(range(15, 256, 16)) + struct.pack('<I', 0)
    sections['layerdef'] = len(out)
    out += name('LAYERDEF') + struct.pack('<II', 36, 1)
    record_at = len(out)
    out += bytes(32)
    if version >= 516:
        sections['extra'] = len(out)
        out += name('EXTRA') + struct.pack('<II', 24, 2) + bytes(24) + struct.pack('<I', 2) + bytes(24)
        sections['machine'] = len(out)
        machine = bytearray(156)
        machine[:12] = name('MACHINE')
        struct.pack_into('<I', machine, 12, 156)
        machine[112:118] = b'pwsimg' if planes else b'pw0Img'
        out += machine
    images = len(out)
    out += image
    struct.pack_into('<IIffffII', out, record_at, images, len(image), 0, 0, 2.0, 0.05, 1, 0)
    slots = [sections['header'], 0, sections['preview'], sections.get('colours', 0), sections['layerdef']]
    if version >= 516:
        slots += [sections['extra'], sections['machine'], images]
    elif version == 515:
        slots += [images, images]
    else:
        slots += [0, images]
    for i, value in enumerate(slots):
        struct.pack_into('<I', out, 20 + 4 * i, value)
    return bytes(out)


class WorkshopValidation(unittest.TestCase):
    def parse(self, data, suffix='.pwma'):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / f'sample{suffix}'
            path.write_bytes(data)
            return validate(path)

    def test_public_layout_decodes(self):
        for version in (1, 515, 516):
            with self.subTest(version=version):
                result = self.parse(build(version))
                self.assertEqual(result['dimensions'], [3, 1])
                self.assertEqual(result['layers'][0]['pixels'], [0, 255, 0])
        planes = build(516, bytes([0x01, 0x81, 0x01]), planes=True)
        self.assertEqual(self.parse(planes)['layers'][0]['pixels'], [0, 255, 0])

    def test_codecs(self):
        self.assertEqual(decode_pw0(bytes([0x00, 0x02, 0x81]), 3), bytes([0, 0, 136]))
        self.assertEqual(decode_planes(bytes([0x82, 0x01, 0x81, 0x02]), 3, 2), (bytes([255, 127, 0]), 3))
        for bad, count in [(bytes([0x80]), 1), (bytes([0x00, 0x05]), 3)]:
            with self.assertRaises(ValueError):
                decode_pw0(bad, count)

    def test_corruption_and_truncation_fail(self):
        good = build(516)
        for data in [good[:-1], good[:40], good.replace(b'LAYERDEF', b'LAYERDEX')]:
            with self.assertRaises((AssertionError, ValueError, struct.error)):
                self.parse(data)
        # A lit-pixel count that disagrees with the image.
        data = bytearray(good)
        record = data.index(b'LAYERDEF') + 20
        struct.pack_into('<I', data, record + 24, 2)
        with self.assertRaises(AssertionError):
            self.parse(bytes(data))


if __name__ == '__main__':
    unittest.main()
