#!/usr/bin/env python3
"""Independently validate raster_reference's default analytic PNG using Python stdlib.

Usage: python3 tools/m4-profile/validate_png.py OUTPUT.png --samples 4
The example's default triangle and rectangular hole are evaluated directly at subpixel
midpoints. No production rasterizer or PNG decoder implementation is imported.
"""
import argparse
import hashlib
import struct
import zlib
from pathlib import Path


RINGS = (((5, 5), (95, 10), (20, 55)), ((20, 15), (35, 15), (35, 25), (20, 25)))


def decode(data):
    """Validate CRCs, constrained chunk order and zlib stream; return dimensions/pixels."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("invalid PNG signature")
    offset, width, height = 8, None, None
    compressed = bytearray()
    kinds = []
    while offset < len(data):
        if offset + 12 > len(data):
            raise ValueError("truncated chunk header")
        size = struct.unpack_from(">I", data, offset)[0]
        end = offset + 12 + size
        if end > len(data):
            raise ValueError("truncated chunk data")
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:end - 4]
        expected_crc = struct.unpack_from(">I", data, end - 4)[0]
        if zlib.crc32(kind + payload) != expected_crc:
            raise ValueError("CRC mismatch")
        if kind == b"IHDR":
            if kinds or size != 13:
                raise ValueError("invalid IHDR position or size")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
            if not width or not height or (depth, color, compression, filtering, interlace) != (8, 0, 0, 0, 0):
                raise ValueError("expected nonempty grayscale8, noninterlaced PNG")
        elif kind == b"IDAT":
            if not kinds or kinds[-1] not in (b"IHDR", b"IDAT"):
                raise ValueError("nonconsecutive IDAT")
            compressed.extend(payload)
        elif kind == b"IEND":
            if size or not kinds or kinds[-1] != b"IDAT" or end != len(data):
                raise ValueError("invalid terminal IEND")
        else:
            raise ValueError(f"unexpected chunk {kind!r}")
        kinds.append(kind)
        offset = end
    if not kinds or kinds[-1] != b"IEND":
        raise ValueError("missing IEND")
    inflater = zlib.decompressobj()
    filtered = inflater.decompress(compressed) + inflater.flush()
    if not inflater.eof or inflater.unused_data or inflater.unconsumed_tail:
        raise ValueError("incomplete or trailing zlib stream")
    stride = width + 1
    if len(filtered) != stride * height:
        raise ValueError("scanline byte count mismatch")
    pixels = bytearray()
    for row in range(height):
        if filtered[row * stride] != 0:
            raise ValueError("expected filter zero")
        pixels.extend(filtered[row * stride + 1:(row + 1) * stride])
    return width, height, pixels


def inside(x, y):
    """Independent even-odd horizontal-ray oracle in physical millimeters."""
    filled = False
    for ring in RINGS:
        for index, (ax, ay) in enumerate(ring):
            bx, by = ring[(index + 1) % len(ring)]
            if (ay > y) != (by > y) and x < ax + (y - ay) * (bx - ax) / (by - ay):
                filled = not filled
    return filled


def validate(path, samples):
    data = Path(path).read_bytes()
    width, height, pixels = decode(data)
    denominator = samples * samples
    for row in range(height):
        for column in range(width):
            count = sum(inside((column + (sx + 0.5) / samples) * 100 / width,
                               (row + (sy + 0.5) / samples) * 60 / height)
                        for sy in range(samples) for sx in range(samples))
            expected = (2 * count * 255 + denominator) // (2 * denominator)
            actual = pixels[row * width + column]
            if actual != expected:
                raise ValueError(f"pixel ({column},{row}): expected {expected}, received {actual}")
    print(f"validated {width}x{height} samples={samples} pixels={len(pixels)} "
          f"intensity_sum={sum(pixels)} png_sha256={hashlib.sha256(data).hexdigest()} "
          f"pixels_sha256={hashlib.sha256(pixels).hexdigest()}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("png")
    parser.add_argument("--samples", type=int, choices=(1, 2, 4, 8, 16), required=True)
    arguments = parser.parse_args()
    validate(arguments.png, arguments.samples)


if __name__ == "__main__":
    main()
