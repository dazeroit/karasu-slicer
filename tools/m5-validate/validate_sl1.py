#!/usr/bin/env python3
"""Independent Prusa SL1/SL1S archive reader (TD-063); no production encoder imports.

Checks the archive members, the 16 config.ini keys (byte-sorted `key = value` lines), every layer
PNG (8-bit grayscale, CRC, zlib, size), the thumbnails (8-bit RGBA) and the totals: the layer count
equals numFast + numSlow, and usedMaterial equals the sampled volume of the layers.
"""
import argparse, hashlib, json, struct, zipfile, zlib
from pathlib import Path

KEYS = ['action', 'expTime', 'expTimeFirst', 'expUserProfile', 'fileCreationTimestamp', 'hollow',
        'jobDir', 'layerHeight', 'numFade', 'numFast', 'numSlow', 'printTime', 'printerModel',
        'printerVariant', 'prusaSlicerVersion', 'usedMaterial']


def png(data):
    """Decode an unfiltered or filter-0 8-bit PNG; return (width, height, channels, pixels)."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at, idat, header = 8, b'', None
    while at < len(data):
        size, kind = struct.unpack_from('>I4s', data, at)
        body = data[at + 8:at + 8 + size]
        assert zlib.crc32(kind + body) == struct.unpack_from('>I', data, at + 8 + size)[0]
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat += body
        at += 12 + size
    width, height, depth, colour = header[:4]
    assert depth == 8
    channels = {0: 1, 2: 3, 6: 4}[colour]
    raw = zlib.decompress(idat)
    stride = width * channels + 1
    assert len(raw) == stride * height
    rows = []
    for y in range(height):
        assert raw[y * stride] == 0, 'filter type'
        rows.append(raw[y * stride + 1:(y + 1) * stride])
    return width, height, channels, b''.join(rows)


def validate(path, resolution, display_mm, sample=None):
    z = zipfile.ZipFile(path)
    names = z.namelist()
    assert len(set(names)) == len(names) and all('..' not in n and not n.startswith('/') for n in names)
    lines = z.read('config.ini').decode('ascii').splitlines()
    pairs = [line.split(' = ', 1) for line in lines]
    keys = [k for k, _ in pairs]
    assert keys == sorted(KEYS, key=lambda k: k.encode()), keys
    config = dict(pairs)
    job = config['jobDir']
    layers = sorted(n for n in names if n.startswith(job) and n.endswith('.png'))
    count = len(layers)
    assert layers == [f'{job}{i:05d}.png' for i in range(count)]
    assert int(config['numFast']) + int(config['numSlow']) == count
    assert 3 <= int(config['numFade']) <= 20 and config['action'] == 'print'
    assert config['printerModel'] in ('SL1', 'SL1S')
    thumbnails = sorted(n for n in names if n.startswith('thumbnail/'))
    for name in thumbnails:
        w, h, channels, _ = png(z.read(name))
        assert channels == 4 and name == f'thumbnail/thumbnail{w}x{h}.png'
    pixel_area = display_mm[0] / resolution[0] * display_mm[1] / resolution[1]
    height = float(config['layerHeight'])
    total = 0
    digest = hashlib.sha256()
    for i, name in enumerate(layers if sample is None else layers[:sample]):
        w, h, channels, pixels = png(z.read(name))
        assert (w, h, channels) == (resolution[0], resolution[1], 1)
        total += sum(pixels)
        digest.update(hashlib.sha256(pixels).digest())
    if sample is None:
        volume = total / 255 * pixel_area * height / 1000
        assert abs(volume - float(config['usedMaterial'])) <= 1e-6 + 1e-9 * volume, (volume, config['usedMaterial'])
    return {'format': config['printerModel'].lower(), 'layers': count, 'thumbnails': thumbnails,
            'config': config, 'layers_sha256': digest.hexdigest()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('path')
    parser.add_argument('--resolution', type=int, nargs=2, required=True)
    parser.add_argument('--display-mm', type=float, nargs=2, required=True)
    parser.add_argument('--sample-layers', type=int)
    args = parser.parse_args()
    print(json.dumps(validate(args.path, args.resolution, args.display_mm, args.sample_layers), indent=2))
