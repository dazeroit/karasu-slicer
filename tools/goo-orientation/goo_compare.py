"""Overlap of two GOO files' stored layers under the four panel flips.

usage: goo_compare.py REFERENCE.goo CANDIDATE.goo LAYER...
For each layer: the candidate's stored image as is, mirrored along columns (X), along rows (Y),
and turned 180 degrees (both), each against the reference's stored image, as intersection over
union of lit pixels on a grid of 4 x 4 pixel cells.
"""
import struct, sys
sys.path.insert(0, __import__('os').path.dirname(__file__))
from goo_layer import decode, layer_offsets
def load(path):
    b = open(path, 'rb').read()
    count, w, h = struct.unpack_from('>IHH', b, 195310)
    return b, count, w, h, list(layer_offsets(b, count))
def cells(img, w, h, c=4):
    cw, ch = w // c, h // c
    out = set()
    for y in range(0, ch * c, c):
        row = y * w
        for x in range(0, cw * c, c):
            if img[row + x] or img[row + x + 2 + 2 * w]:
                out.add((x // c, y // c))
    return out, cw, ch
ref, can = load(sys.argv[1]), load(sys.argv[2])
assert ref[2:4] == can[2:4], 'panels differ'
w, h = ref[2], ref[3]
for layer in map(int, sys.argv[3:]):
    a, cw, ch = cells(decode(ref[0][ref[4][layer][0] + 70:][:ref[4][layer][1]], w, h), w, h)
    b, _, _ = cells(decode(can[0][can[4][layer][0] + 70:][:can[4][layer][1]], w, h), w, h)
    flips = {'as stored': lambda p: p, 'mirror X': lambda p: (cw - 1 - p[0], p[1]),
             'mirror Y': lambda p: (p[0], ch - 1 - p[1]), 'turned 180': lambda p: (cw - 1 - p[0], ch - 1 - p[1])}
    res = {k: len(a & set(map(f, b))) / max(1, len(a | set(map(f, b)))) for k, f in flips.items()}
    print(f'layer {layer}: ' + ', '.join(f'{k} {v:.2f}' for k, v in res.items()))
