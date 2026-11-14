"""A watertight "F" prism for panel-orientation checks, as a binary STL (millimetres).

usage: make_f.py [OUT.stl]   (default F_orientation_test.stl)

Seen from above (+Z toward the viewer, +X right, +Y toward the plate's back) the letter reads
correctly: a 6 mm stem on the left, a 20 mm top arm and a 16 mm middle arm pointing right. The
letter is chiral, so each of the panel's flips and turns gives a different image. The top and
bottom faces are triangulated from six rectangles whose shared edges match exactly, and the side
walls follow every boundary vertex, so the mesh has no T-junction.
"""
import struct, sys

THICKNESS = 2.0
# Rectangles (x0, y0, x1, y1) covering the F without overlap; shared edges match.
RECTS = [(0, 0, 6, 12), (0, 12, 6, 18), (0, 18, 6, 24), (0, 24, 6, 30),
         (6, 12, 16, 18), (6, 24, 20, 30)]
# The outline, counter-clockwise seen from above, with every vertex of the faces on it.
OUTLINE = [(0, 0), (6, 0), (6, 12), (16, 12), (16, 18), (6, 18), (6, 24), (20, 24), (20, 30),
           (6, 30), (0, 30), (0, 24), (0, 18), (0, 12)]

def tri(a, b, c):
    ux, uy, uz = (b[i] - a[i] for i in range(3))
    vx, vy, vz = (c[i] - a[i] for i in range(3))
    n = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    length = sum(v * v for v in n) ** 0.5
    return tuple(v / length for v in n), a, b, c

def triangles():
    out = []
    for x0, y0, x1, y1 in RECTS:
        for z, up in ((THICKNESS, True), (0.0, False)):
            p = [(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)]
            if up:
                out += [tri(p[0], p[1], p[2]), tri(p[0], p[2], p[3])]
            else:
                out += [tri(p[0], p[2], p[1]), tri(p[0], p[3], p[2])]
    for (ax, ay), (bx, by) in zip(OUTLINE, OUTLINE[1:] + OUTLINE[:1]):
        a0, b0, a1, b1 = (ax, ay, 0.0), (bx, by, 0.0), (ax, ay, THICKNESS), (bx, by, THICKNESS)
        out += [tri(a0, b0, b1), tri(a0, b1, a1)]
    return out

def write(path):
    tris = triangles()
    with open(path, 'wb') as f:
        f.write(b'karasu-slicer orientation test F, 20 x 30 x 2 mm'.ljust(80, b' '))
        f.write(struct.pack('<I', len(tris)))
        for n, a, b, c in tris:
            f.write(struct.pack('<12fH', *n, *a, *b, *c, 0))
    return len(tris)

if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else 'F_orientation_test.stl'
    print(out, write(out), 'triangles')
