"""Tolerant readers of resin printer files, for orientation comparisons.

Each reader takes from a file, written by any slicer, the panel's size, the layer count, one
layer's pixels (one byte per pixel, 0 unlit) and what the file declares about its stored image's
orientation. They check nothing else, so that other slicers' files load too. Supported: GOO V3,
the Anycubic Photon Workshop family (`.pwmx`, `.pwma`, `.pm3`, `.pws` and the others, with PW0 or
bit-plane images), PP1, the original Photon's `.photon` and plain CTB (encrypted CTB is not read).
"""
import json
import struct
import sys
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / 'goo-orientation'))
import goo_layer  # noqa: E402  (GOO V3 layer decoding, shared with tools/goo-orientation)


class PrintFile:
    """A file's panel, layer count, declared orientation and a way to decode one layer."""

    def __init__(self, path, software, width, height, count, declared, layer):
        self.path, self.software = Path(path), software
        self.width, self.height, self.count = width, height, count
        self.declared, self.layer = declared, layer


def decode_pw0(data, count):
    """PW0 runs (4-bit level, 4- or 12-bit length); a truncated end leaves the rest unlit."""
    out = bytearray(count)
    at = j = 0
    while j < len(data) and at < count:
        b = data[j]
        j += 1
        level, run = b >> 4, b & 15
        if level in (0, 15):
            if j >= len(data):
                break
            run = run << 8 | data[j]
            j += 1
        if level:
            n = min(run, count - at)
            out[at:at + n] = bytes([level * 17]) * n
        at += run
    return bytes(out)


def decode_planes(data, count, planes):
    """`.pws` bit-planes (7-bit runs, high bit lit); a pixel lit in any plane is lit."""
    out = bytearray(count)
    at = 0
    for _ in range(planes):
        filled = 0
        while filled < count and at < len(data):
            b = data[at]
            at += 1
            run = min(b & 127, count - filled)
            if b & 128:
                out[filled:filled + run] = b'\xff' * run
            filled += run
    return bytes(out)


def read_goo(path, b):
    count, width, height = struct.unpack_from('>IHH', b, 195310)
    mirror_x, mirror_y = b[195318], b[195319]
    software = b[12:44].split(b'\0')[0].decode('latin-1')
    offsets = list(goo_layer.layer_offsets(b, count))

    def layer(i):
        at, n = offsets[i]
        return bytes(goo_layer.decode(b[at + 70:at + 70 + n], width, height))
    return PrintFile(path, software, width, height, count,
                     {'mirror_x': mirror_x, 'mirror_y': mirror_y}, layer)


def read_workshop(path, b):
    u32 = lambda at: struct.unpack_from('<I', b, at)[0]
    version = u32(12)
    slot = lambda i: u32(20 + 4 * i)
    header = slot(0)
    planes, width, height = u32(header + 56), u32(header + 60), u32(header + 64)
    layerdef = slot(4)
    count = u32(layerdef + 16)
    if version >= 516:
        machine = slot(6)
        image_format = b[machine + 112:machine + 128].split(b'\0')[0].decode('latin-1')
    else:
        image_format = 'pwsimg' if Path(path).suffix.lower() == '.pws' else 'pw0Img'

    def layer(i):
        record = layerdef + 20 + 32 * i
        data = b[u32(record):u32(record) + u32(record + 4)]
        if image_format == 'pwsimg':
            return decode_planes(data, width * height, max(1, planes))
        return decode_pw0(data, width * height)
    return PrintFile(path, f'Photon Workshop family v{version} ({image_format})', width, height,
                     count, {}, layer)


def read_chitu(path, b):
    """`.photon` (magic 0x12FD0019) and plain CTB (0x12FD0086, 0x12FD0106)."""
    u32 = lambda at: struct.unpack_from('<I', b, at)[0]
    magic, version = u32(0), u32(4)
    width, height = u32(52), u32(56)
    table, count = u32(64), u32(68)
    photon = magic == 0x12FD0019

    def layer(i):
        offset, n = struct.unpack_from('<II', b, table + 36 * i + 12)
        data = b[offset:offset + n]
        out = bytearray(width * height)
        at = j = 0
        while j < len(data) and at < len(out):
            code = data[j]
            j += 1
            if photon:
                lit, run = code & 128, code & 127
            else:
                lit, run = code & 127, 1
                if code & 128:
                    q = data[j]
                    j += 1
                    extra, run = ((0, q) if q < 128 else (1, q & 63) if q < 192 else
                                  (2, q & 31) if q < 224 else (3, q & 15))
                    for _ in range(extra):
                        run = run * 256 + data[j]
                        j += 1
            n_run = min(run, len(out) - at)
            if lit:
                out[at:at + n_run] = b'\xff' * n_run
            at += run
        return bytes(out)
    name = 'Photon v1' if photon else f'CTB v{version} (plain)'
    return PrintFile(path, name, width, height, count, {}, layer)


def read_pp1(path):
    archive = zipfile.ZipFile(path)
    manifest = json.loads(archive.read('anycubic_photon_resins.pwsp'))
    machine = manifest['machine_type']
    width, height = machine['res_x'], machine['res_y']
    count = len(json.loads(archive.read('layers_controller.conf'))['paras'])
    declared = {'rotate_z': machine.get('rotate_z')}
    software = ''
    if 'software_info.conf' in archive.namelist():
        info = json.loads(archive.read('software_info.conf'))
        software = f"{info.get('mark', '')} {info.get('version', '')}".strip()

    def layer(i):
        return decode_pw0(archive.read(f'layer_images/layer_{i}.pw0Img'), width * height)
    return PrintFile(path, software, width, height, count, declared, layer)


def read(path):
    """The print file at `path`, whatever slicer wrote it."""
    path = Path(path)
    if path.suffix.lower() == '.pp1':
        return read_pp1(path)
    b = path.read_bytes()
    if b[:4] == b'V3.0':
        return read_goo(path, b)
    if b[:8] == b'ANYCUBIC':
        return read_workshop(path, b)
    if struct.unpack_from('<I', b, 0)[0] in (0x12FD0019, 0x12FD0086, 0x12FD0106):
        return read_chitu(path, b)
    raise ValueError(f'{path.name}: not a GOO V3, Photon Workshop, PP1, .photon or plain CTB file')
