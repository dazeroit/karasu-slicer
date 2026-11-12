"""Public-layout fixtures independent of the Rust writer and codec helpers."""
import struct
import tempfile
import unittest
from pathlib import Path
from validate_machine import validate, validate_with_previews


def legacy(magic):
    output = bytearray(112 + 36)
    struct.pack_into('<II', output, 0, magic, 2 if magic == 0x12fd0086 else 1)
    struct.pack_into('<II', output, 52, 3, 1)
    struct.pack_into('<II', output, 64, 112, 1)
    image = bytes([0, 127, 0]) if magic == 0x12fd0086 else bytes([1, 129, 1])
    struct.pack_into('<fffII', output, 112, .05, 2., 0., len(output), len(image))
    return bytes(output) + image


def legacy_with_previews(magic):
    """Large 2 x 1 white run and small 1 x 1 red pixel, then the plain layer table."""
    header = bytearray(112)
    struct.pack_into('<II', header, 0, magic, 2 if magic == 0x12fd0086 else 1)
    struct.pack_into('<II', header, 52, 3, 1)
    large = struct.pack('<HH', 0xffdf | 0x20, 0x3000 | 1)
    small = struct.pack('<H', 0xf800)
    output = bytearray(header)
    for at, (width, height, data) in [(60, (2, 1, large)), (72, (1, 1, small))]:
        descriptor = len(output)
        struct.pack_into('<I', output, at, descriptor)
        output += struct.pack('<IIII', width, height, descriptor + 32, len(data)) + bytes(16) + data
    table = len(output)
    struct.pack_into('<II', output, 64, table, 1)
    image = bytes([0, 127, 0]) if magic == 0x12fd0086 else bytes([1, 129, 1])
    output += struct.pack('<fffII', .05, 2., 0., table + 36, len(image)) + bytes(16)
    return bytes(output) + image


def goo():
    output = bytearray(195477)
    magic = bytes.fromhex('07000000444c5000')
    output[:12] = b'V3.0' + magic
    output[27106:27108] = b'\r\n'
    output[195308:195310] = b'\r\n'
    struct.pack_into('>IHH', output, 195310, 1, 3, 1)
    struct.pack_into('>I', output, 195470, 195477)
    record = bytearray(70)
    struct.pack_into('>Hfff', record, 0, 0, 10., .05, 2.)
    record[64:66] = b'\r\n'
    image = bytes([85, 1, 193, 1, 60])
    struct.pack_into('>I', record, 66, len(image))
    return bytes(output) + record + image + b'\r\n' + bytes(3) + magic


class PublicProtocolValidation(unittest.TestCase):
    def parse(self, data):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sample'
            path.write_bytes(data)
            return validate(path)

    def test_three_independent_protocol_vectors(self):
        for data in [legacy(0x12fd0019), legacy(0x12fd0086), goo()]:
            with self.subTest(magic=data[:4]):
                result = self.parse(data)
                self.assertEqual(result['dimensions'], [3, 1])
                self.assertEqual(result['layers'][0]['pixels'], [0, 255, 0])

    def test_rendered_previews_decode(self):
        for magic in [0x12fd0019, 0x12fd0086]:
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'sample'
                path.write_bytes(legacy_with_previews(magic))
                summary, previews = validate_with_previews(path)
            self.assertEqual(summary['layers'][0]['pixels'], [0, 255, 0])
            self.assertEqual(previews, [(1, 1, bytes([255, 0, 0])), (2, 1, bytes([255] * 6))])
        data = bytearray(goo())
        struct.pack_into('>H', data, 194, 0x07e0)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sample'
            path.write_bytes(data)
            summary, previews = validate_with_previews(path)
        self.assertEqual(previews[0][2][:6], bytes([0, 255, 0, 0, 0, 0]))
        self.assertEqual(summary['previews'][0]['nonblack_pixels'], 1)
        self.assertEqual([p['dimensions'] for p in summary['previews']], [[116, 116], [290, 290]])

    def test_malformed_preview_runs_fail(self):
        data = bytearray(legacy_with_previews(0x12fd0086))
        descriptor = struct.unpack_from('<I', data, 60)[0]
        # The run word loses its 0x3000 marker.
        struct.pack_into('<H', data, descriptor + 34, 1)
        with self.assertRaises(AssertionError):
            self.parse(data)

    def test_truncation_and_offset_corruption_fail(self):
        for original in [legacy(0x12fd0019), legacy(0x12fd0086), goo()]:
            for cut in [0, 4, 20, len(original)-1]:
                with self.subTest(size=cut):
                    with self.assertRaises((AssertionError, ValueError, IndexError, struct.error)):
                        self.parse(original[:cut])
        data = bytearray(legacy(0x12fd0086))
        struct.pack_into('<I', data, 124, 0xfffffff0)
        with self.assertRaises(ValueError):
            self.parse(data)

    def test_goo_checksum_and_delimiter_corruption_fail(self):
        for index in [195477 + 70 + 4, 195477 + 64]:
            data = bytearray(goo())
            data[index] ^= 1
            with self.assertRaises(AssertionError):
                self.parse(data)


if __name__ == '__main__':
    unittest.main()
