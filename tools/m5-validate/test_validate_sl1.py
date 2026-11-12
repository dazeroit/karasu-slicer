"""Hand-built SL1 archives, independent of the Rust writer."""
import io
import struct
import tempfile
import unittest
import zipfile
import zlib
from pathlib import Path

from validate_sl1 import KEYS, png, validate


def make_png(width, height, channels, pixels):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    colour = {1: 0, 3: 2, 4: 6}[channels]
    stride = width * channels
    raw = b''.join(b'\0' + pixels[y * stride:(y + 1) * stride] for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, colour, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))


def archive(layers, overrides=None):
    values = {'action': 'print', 'expTime': '2', 'expTimeFirst': '20', 'expUserProfile': '0',
              'fileCreationTimestamp': 'x', 'hollow': '0', 'jobDir': 'part', 'layerHeight': '0.05',
              'numFade': '3', 'numFast': str(len(layers)), 'numSlow': '0', 'printTime': '1.000000',
              'printerModel': 'SL1S', 'printerVariant': 'default', 'prusaSlicerVersion': 'test',
              'usedMaterial': '%.6f' % (sum(sum(l) for l in layers) / 255 * 1 * 0.05 / 1000)}
    values.update(overrides or {})
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, 'w') as z:
        for i, layer in enumerate(layers):
            z.writestr(f'part{i:05d}.png', make_png(3, 1, 1, layer))
        z.writestr('thumbnail/thumbnail2x1.png', make_png(2, 1, 4, bytes(8)))
        z.writestr('config.ini', ''.join(f'{k} = {values[k]}\n' for k in sorted(values, key=str.encode)))
    return buffer.getvalue()


class Sl1Validation(unittest.TestCase):
    def parse(self, data):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sample.sl1s'
            path.write_bytes(data)
            return validate(path, [3, 1], [3, 1])

    def test_archive_validates(self):
        result = self.parse(archive([bytes([0, 255, 0]), bytes([255, 255, 0])]))
        self.assertEqual(result['layers'], 2)
        self.assertEqual(png(make_png(3, 1, 1, bytes([1, 2, 3])))[3], bytes([1, 2, 3]))
        self.assertEqual(len(KEYS), 16)

    def test_wrong_totals_and_keys_fail(self):
        layers = [bytes([0, 255, 0])]
        for overrides in [{'numFast': '5'}, {'usedMaterial': '1.000000'}, {'numFade': '2'}, {'printerModel': 'X'}]:
            with self.subTest(overrides=overrides), self.assertRaises(AssertionError):
                self.parse(archive(layers, overrides))


if __name__ == '__main__':
    unittest.main()
