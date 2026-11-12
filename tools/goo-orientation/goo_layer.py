"""Decode one GOO V3 layer to a PNG of its lit region, as stored.

usage: goo_layer.py FILE.goo LAYER OUT.png [SCALE]
Row 0 of the PNG is the stored image's first row; nothing is flipped.
"""
import struct, sys, zlib
def layer_offsets(b, count):
    p = 195477
    for _ in range(count):
        n = struct.unpack_from('>I', b, p + 66)[0]
        yield p, n
        p += 72 + n
def decode(data, w, h):
    out = bytearray(w * h); j = 1; prev = 0; at = 0
    while j < len(data) - 1:
        tag = data[j]; j += 1; kind = tag >> 6
        if kind == 2:
            value = prev + (-1 if tag & 32 else 1) * (tag & 15)
            run = data[j] if tag & 16 else 1
            j += bool(tag & 16)
        else:
            value = (0, 0, 0, 255)[kind]
            if kind == 1:
                value = data[j]; j += 1
            run = tag & 15; extra = (tag >> 4) & 3
            for k in range(extra):
                run |= data[j + k] << (4 + 8 * (extra - k - 1))
            j += extra
        if value:
            out[at:at + run] = bytes([value]) * run
        at += run; prev = value
    assert at == w * h, (at, w * h)
    return out
def png(path, w, h, rows):
    raw = b''.join(b'\0' + bytes(r) for r in rows)
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d))
    open(path, 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 0, 0, 0, 0))
                           + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b''))
def main():
    path, index, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    scale = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    b = open(path, 'rb').read()
    count, w, h = struct.unpack_from('>IHH', b, 195310)
    for i, (p, n) in enumerate(layer_offsets(b, count)):
        if i == index:
            img = decode(b[p + 70:p + 70 + n], w, h); break
    lit = [(i % w, i // w) for i in range(0, w * h, 7) if img[i]]
    xs = [x for x, _ in lit]; ys = [y for _, y in lit]
    x0, x1, y0, y1 = max(min(xs) - 40, 0), min(max(xs) + 40, w), max(min(ys) - 40, 0), min(max(ys) + 40, h)
    rows = []
    for y in range(y0, y1, scale):
        rows.append(bytes(img[y * w + x] for x in range(x0, x1, scale)))
    png(out, len(rows[0]), len(rows), rows)
    print(f'{path}: layer {index}/{count}, {w}x{h}, lit box x {x0}-{x1} y {y0}-{y1}, png {len(rows[0])}x{len(rows)}')
if __name__ == "__main__":
    main()
