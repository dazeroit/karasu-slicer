#!/usr/bin/env python3
"""Independent Anycubic Photon Workshop reader (TD-063); no production encoder imports.

Reads the ANYCUBIC file mark and address table, checks every section's name, length and
position, decodes each layer (PW0 levels or .pws bit-planes) and checks the LAYERDEF records
(address, length, thickness, lit-pixel count) against the decoded images.
"""
import hashlib, json, struct, sys
from pathlib import Path

MARKS = {1: (48, 4), 515: (48, 5), 516: (52, 8), 517: (56, 9), 518: (64, 11)}
HEADER_BYTES = {1: 96, 515: 96, 516: 100, 517: 108, 518: 112}


def decode_pw0(data, count):
    out = bytearray()
    j = 0
    while j < len(data):
        b = data[j]; j += 1
        level = b >> 4
        run = b & 15
        if level in (0, 15):
            if j >= len(data):
                raise ValueError('truncated PW0 run')
            run = run << 8 | data[j]; j += 1
        if run == 0 or len(out) + run > count:
            raise ValueError('invalid PW0 run')
        out.extend(bytes([level * 17]) * run)
    if len(out) != count:
        raise ValueError('PW0 pixel count')
    return bytes(out)


def decode_planes(data, count, planes):
    lit = [0] * count
    at = 0
    total_lit = 0
    for plane in range(planes):
        filled = 0
        while filled < count:
            if at >= len(data):
                raise ValueError('truncated bit-plane')
            b = data[at]; at += 1
            run = b & 127
            if run == 0 or filled + run > count:
                raise ValueError('invalid bit-plane run')
            if b & 128:
                for k in range(filled, filled + run):
                    lit[k] += 1
                total_lit += run
            filled += run
    if at != len(data):
        raise ValueError('bit-plane trailing bytes')
    out = bytes(0 if c == 0 else c * 256 // planes - 1 for c in lit)
    return out, total_lit


def validate(path, decode_layers=True):
    b = Path(path).read_bytes()

    def u32(at):
        if at < 0 or at + 4 > len(b):
            raise ValueError('truncated field')
        return struct.unpack_from('<I', b, at)[0]

    def f32(at):
        if at < 0 or at + 4 > len(b):
            raise ValueError('truncated field')
        return struct.unpack_from('<f', b, at)[0]

    def tag(at, expected):
        assert b[at:at + 12] == expected.encode().ljust(12, b'\0'), (at, expected)

    assert b[:12] == b'ANYCUBIC\0\0\0\0'
    version, areas = u32(12), u32(16)
    assert version in MARKS and MARKS[version][1] == areas, (version, areas)
    slot = lambda i: u32(20 + 4 * i)
    header = slot(0)
    assert header == MARKS[version][0]
    tag(header, 'HEADER')
    assert u32(header + 12) == HEADER_BYTES[version] - 16
    width, height = u32(header + 60), u32(header + 64)
    aa = u32(header + 56)
    assert 0 < width * height <= 128 * 1024 * 1024 and aa >= 1
    preview = slot(2)
    tag(preview, 'PREVIEW')
    pw, dpi, ph = u32(preview + 16), u32(preview + 20), u32(preview + 24)
    assert (pw, ph) == (224, 168) and u32(preview + 12) == pw * ph * 2 + 12
    if version >= 515:
        colours = slot(3)
        assert u32(colours + 4) == 16 and b[colours + 8:colours + 24] == bytes(range(15, 256, 16))
    layerdef = slot(4)
    tag(layerdef, 'LAYERDEF')
    count = u32(layerdef + 16)
    assert u32(layerdef + 12) == count * 32 + 4 and 0 < count <= 1_000_000
    image_format = 'pw0Img'
    if version >= 516:
        extra = slot(5)
        tag(extra, 'EXTRA')
        assert u32(extra + 12) == 24 and u32(extra + 16) == 2 and u32(extra + 44) == 2
        machine = slot(6)
        tag(machine, 'MACHINE')
        size = 224 if version == 518 else 156
        assert u32(machine + 12) == size
        image_format = b[machine + 112:machine + 128].rstrip(b'\0').decode()
        assert image_format in ('pw0Img', 'pwsimg')
        images = slot(7)
    else:
        images = slot(6)
    if version >= 517:
        software = slot(1)
        assert b[software:software + 13] == b'karasu-slicer' and u32(software + 32) == 164
        tag(slot(8), 'MODEL')
    sub = None
    if version == 518:
        sub = slot(9)
        tag(sub, 'SUBIMGS')
        assert u32(sub + 16) == count and u32(sub + 12) == count * 44 + 24
    bitplanes = image_format == 'pwsimg' or Path(path).suffix == '.pws'
    layers = []
    expected = images
    for i in range(count):
        record = layerdef + 20 + 32 * i
        address, length = u32(record), u32(record + 4)
        assert address == expected, (i, address, expected)
        expected = address + length
        thickness, lit = f32(record + 20), u32(record + 24)
        assert thickness > 0
        if sub is not None:
            s = sub + 24 + 44 * i
            assert (u32(s), u32(s + 4), u32(s + 8)) == (address, length, lit)
        if decode_layers:
            data = b[address:address + length]
            assert len(data) == length
            if bitplanes:
                pixels, total = decode_planes(data, width * height, aa)
                assert total == lit, (i, total, lit)
            else:
                pixels = decode_pw0(data, width * height)
                assert sum(1 for v in pixels if v) == lit, i
            layers.append({'exposure': f32(record + 16), 'thickness': thickness,
                           'pixels': list(pixels) if width * height <= 4096 else None,
                           'mask_sha256': hashlib.sha256(pixels).hexdigest()})
    assert expected == len(b), 'trailing bytes after the last image'
    return {'format': f'anycubic-v{version}', 'image_format': image_format,
            'dimensions': [width, height], 'antialiasing': aa, 'layers': layers,
            'volume_ml': f32(header + 52)}


if __name__ == '__main__':
    print(json.dumps(validate(sys.argv[1]), indent=2))
