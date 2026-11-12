#!/usr/bin/env python3
"""Generate the interface's icons, assets/ui/icons/<name>.svg and <name>.png (karasu-slicer).

Original, drawn here (owner decision, M6.5): each icon is a few strokes on a 24-unit grid, the
way line icons are drawn, with round caps and joins. For each icon the script writes:
- <name>.svg, the editable source, stroked in white;
- <name>.png, 96 x 96 pixels, white with the strokes' coverage as alpha. The application tints
  it with the theme's colours, so one image serves every state (normal, chosen, disabled).

Deterministic and dependency-free. Coverage is analytic: a pixel's alpha is how far its centre
lies inside the stroke, smoothed over one pixel, so edges are anti-aliased without sampling.

Usage: make_icons.py <output folder>
"""
import math, os, struct, sys, zlib

GRID = 24.0             # the icons' design grid
SIZE = 96               # pixels per side of each PNG (4 per grid unit)
STROKE = 1.6            # stroke width, in grid units
ARC_STEPS = 96          # segments per full turn of an arc


def line(a, b):
    return [a, b]


def arc(centre, radius, start_deg, end_deg):
    """Points along an arc, angles in degrees with y pointing down (screen coordinates)."""
    turns = abs(end_deg - start_deg) / 360.0
    steps = max(2, int(math.ceil(ARC_STEPS * turns)))
    return [
        (centre[0] + radius * math.cos(math.radians(start_deg + (end_deg - start_deg) * k / steps)),
         centre[1] + radius * math.sin(math.radians(start_deg + (end_deg - start_deg) * k / steps)))
        for k in range(steps + 1)
    ]


def closed(points):
    return points + [points[0]]


def capsule(centre, angle_deg, half, radius):
    """A closed capsule: two half circles of `radius` joined, `half` from `centre` along
    `angle_deg` each way (a chain link)."""
    a = math.radians(angle_deg)
    u = (math.cos(a), math.sin(a))
    ends = [(centre[0] + u[0] * half, centre[1] + u[1] * half),
            (centre[0] - u[0] * half, centre[1] - u[1] * half)]
    points = arc(ends[0], radius, angle_deg - 90, angle_deg + 90)
    points += arc(ends[1], radius, angle_deg + 90, angle_deg + 270)
    return closed(points)


def drop():
    """A drop: a circle's lower arc and its two tangents meeting at the top."""
    tip, centre, radius = (12.0, 3.0), (12.0, 14.5), 5.5
    distance = centre[1] - tip[1]
    half = math.degrees(math.acos(radius / distance))
    start, end = -90.0 + half, 270.0 - half
    body = arc(centre, radius, start, end)
    return [[tip] + body + [tip]]


def wrench():
    """A wrench: a head open towards the top right, and a handle to the bottom left."""
    centre, radius = (15.5, 8.5), 5.0
    head = arc(centre, radius, -5.0, 275.0)
    # The handle's two sides leave the head along its diagonal and end in a rounded tip.
    direction = (-math.sqrt(0.5), math.sqrt(0.5))
    normal = (math.sqrt(0.5), math.sqrt(0.5))
    width = 1.7
    start = (centre[0] + direction[0] * (radius - 0.6), centre[1] + direction[1] * (radius - 0.6))
    end = (5.0, 19.0)
    side = lambda p, s: (p[0] + normal[0] * width * s, p[1] + normal[1] * width * s)
    tip = arc(end, width, 45.0, 225.0)
    return [head, [side(start, 1), side(end, 1)], [side(start, -1), side(end, -1)], tip]


ICONS = {
    # Transform panel (M6.5): turning, scaling, locking, resetting, linking the axes.
    'rotate': [arc((12, 12), 8, -20, -340), [(19.8, 5.5), (19.5, 9.3), (15.8, 8.6)]],
    'scale': [line((5, 19), (19, 5)), [(13, 5), (19, 5), (19, 11)], [(5, 13), (5, 19), (11, 19)]],
    'lock-open': [closed([(5, 11), (19, 11), (19, 21), (5, 21)]), [(8, 11), (8, 7)] + arc((12, 7), 4, 180, 330)],
    'lock-closed': [closed([(5, 11), (19, 11), (19, 21), (5, 21)]), [(8, 11), (8, 7)] + arc((12, 7), 4, 180, 360) + [(16, 11)]],
    'reset': [arc((12, 12), 8, 200, 520), [(4.2, 5.5), (4.5, 9.3), (8.2, 8.6)]],
    'link': [capsule((9.2, 14.8), -45, 2.6, 3.4), capsule((14.8, 9.2), -45, 2.6, 3.4)],
    # A pointer: selecting, with no handles.
    'select': [
        closed([(6, 3), (6, 19), (10.4, 15.2), (13.4, 21.4), (15.8, 20.3), (12.9, 14.2), (18.6, 13.8)]),
    ],
    # Four arrows: moving, turning and scaling the object.
    'transform': [
        line((12, 3), (12, 21)), line((3, 12), (21, 12)),
        [(9, 6), (12, 3), (15, 6)], [(9, 18), (12, 21), (15, 18)],
        [(6, 9), (3, 12), (6, 15)], [(18, 9), (21, 12), (18, 15)],
    ],
    'repair': wrench(),
    # The Orient tool (TD-119): a model tilted on its corner on the plate, and the turn above it.
    'orient': [
        line((3, 21), (21, 21)),
        closed([(12, 21), (6.5, 15.5), (12, 10), (17.5, 15.5)]),
        arc((12, 12), 9, 200, 340), [(19.4, 4.9), (20.5, 8.9), (16.6, 8.6)],
    ],
    # A protected point (TD-119): a shield with a check.
    'protect': [
        closed([(12, 3), (19.5, 6), (19.5, 11.5), (16.5, 17), (12, 21), (7.5, 17), (4.5, 11.5), (4.5, 6)]),
        [(8.5, 12), (11, 14.5), (15.5, 9.5)],
    ],
    # A cube's shell with a smaller cube inside: the hollowed body.
    'hollow': [
        closed([(12, 2.5), (20.5, 7), (20.5, 17), (12, 21.5), (3.5, 17), (3.5, 7)]),
        [(3.5, 7), (12, 11.5), (20.5, 7)], line((12, 11.5), (12, 21.5)),
    ],
    'drain-holes': drop(),
    # A support (TD-108): the model's underside, the tip's cone, the shaft and the pad.
    'supports': [
        line((4, 3.5), (20, 3.5)),
        [(12, 3.5), (14, 9), (10, 9), (12, 3.5)], line((12, 9), (12, 17.5)),
        [(9.5, 20.5), (12, 17.5), (14.5, 20.5)], line((6.5, 20.5), (17.5, 20.5)),
    ],
    # Stacked layers: slicing into layers, and exporting them.
    # Notification severities (M6.5): a circle with i, a check, or a cross; a triangle with !.
    'info': [arc((12, 12), 9, 0, 360), line((12, 11), (12, 16.5)), line((12, 7.6), (12, 7.7))],
    'success': [arc((12, 12), 9, 0, 360), [(7.8, 12.3), (10.8, 15.2), (16.3, 8.8)]],
    'warning': [
        closed([(12, 3), (21.5, 19.5), (2.5, 19.5)]),
        line((12, 9.5), (12, 13.8)), line((12, 16.6), (12, 16.7)),
    ],
    'error': [arc((12, 12), 9, 0, 360), line((9, 9), (15, 15)), line((15, 9), (9, 15))],
    # Four corners pointing out: a larger view.
    'enlarge': [
        [(4, 9), (4, 4), (9, 4)], [(15, 4), (20, 4), (20, 9)],
        [(20, 15), (20, 20), (15, 20)], [(9, 20), (4, 20), (4, 15)],
    ],
    # One layer seen from above: the plate's frame and a cross-section in it.
    'layers': [
        closed([(3.5, 3.5), (20.5, 3.5), (20.5, 20.5), (3.5, 20.5)]),
        arc((12, 12), 5, 0, 360), arc((12, 12), 2.2, 0, 360),
    ],
    # Library rows (TD-106): editing (a pencil), duplicating (two sheets), exporting (an arrow
    # out of a tray) and deleting (a bin).
    'edit': [
        closed([(4, 20), (5, 15.5), (15.5, 5), (19, 8.5), (8.5, 19)]),
        line((13.2, 7.3), (16.7, 10.8)),
    ],
    'duplicate': [
        closed([(9, 9), (20, 9), (20, 20), (9, 20)]),
        [(15, 9), (15, 4), (4, 4), (4, 15), (9, 15)],
    ],
    'export': [
        [(4, 14), (4, 20), (20, 20), (20, 14)],
        line((12, 3.5), (12, 15)), [(7.5, 8), (12, 3.5), (16.5, 8)],
    ],
    # The tool strip (owner, 2026-10-29): where supports and holes go (the outside, or the
    # cavity: a shell around a void), supports going round the model, a knee following the foot,
    # and the overhangs (a ledge with the strokes under it).
    'surface-outside': [arc((12, 12), 8.5, 0, 360)],
    'surface-cavity': [arc((12, 12), 8.5, 0, 360), arc((12, 12), 4, 0, 360)],
    # Only the supports' tips (owner, 2026-10-29): three tips, their shafts dotted away.
    'tips-only': [
        closed([(3.5, 4), (8.5, 4), (6, 9)]), closed([(9.5, 4), (14.5, 4), (12, 9)]),
        closed([(15.5, 4), (20.5, 4), (18, 9)]),
        line((6, 13), (6, 14.5)), line((12, 13), (12, 14.5)), line((18, 13), (18, 14.5)),
        line((6, 18.5), (6, 20)), line((12, 18.5), (12, 20)), line((18, 18.5), (18, 20)),
    ],
    # Mini supports (owner, 2026-10-30): a thin rod with a ball at each end, between an underside
    # and the model below.
    'mini': [
        line((3, 3.5), (21, 3.5)), line((3, 20.5), (21, 20.5)),
        arc((12, 7), 1.7, 0, 360), line((12, 8.7), (12, 15.3)), arc((12, 17), 1.7, 0, 360),
    ],
    # Suction cups (M6.4j, TD-114): a cup's dome on its rim, and its stem.
    'suction-cups': [
        line((12, 3), (12, 9.5)), arc((12, 18), 8, 180, 360), line((3, 18), (21, 18)),
    ],
    # Hollowing blockers (owner mockup, 2026-11-02): a truncated pyramid, with a plus to place one.
    'blockers': [
        closed([(6, 20), (18, 20), (16, 11), (8, 11)]),
        line((12, 3), (12, 8)), line((9.5, 5.5), (14.5, 5.5)),
    ],
    # The Raft tool (TD-108): pads standing on a slab.
    'raft': [
        line((7, 4), (7, 14)), line((12, 4), (12, 14)), line((17, 4), (17, 14)),
        closed([(3, 17), (21, 17), (19, 20.5), (5, 20.5)]),
    ],
    # The Braces tool (owner, 2026-10-29): two shafts and a brace between them; a chain of
    # braces across three shafts; braces placed automatically, zigzagging between two shafts.
    'braces': [line((6, 3), (6, 21)), line((18, 3), (18, 21)), line((6, 8), (18, 14))],
    'braces-chain': [
        line((4, 3), (4, 21)), line((12, 3), (12, 21)), line((20, 3), (20, 21)),
        [(4, 9), (12, 13), (20, 9)],
    ],
    'braces-auto': [
        line((5, 3), (5, 21)), line((19, 3), (19, 21)),
        [(5, 6), (19, 10), (5, 14), (19, 18)],
    ],
    # The Islands tool (owner, 2026-10-29): a blob in mid-air over a wavy floor; local minima, a
    # diamond over the plate.
    'islands': [
        [(3, 18), (6, 17), (9, 17.5), (12, 18.5), (15, 18.5), (18, 17.5), (21, 18)],
        arc((12, 9), 4.5, 0, 360),
    ],
    'minima': [closed([(12, 5), (17, 10), (12, 15), (7, 10)]), line((4, 20), (20, 20))],
    # Supporting the islands: a blob in mid-air held by a support from the plate.
    'support-islands': [
        arc((12, 5.5), 3.5, 0, 360), line((12, 9), (12, 18)), line((7, 20.5), (17, 20.5)),
        [(9.5, 20.5), (12, 18), (14.5, 20.5)],
    ],
    # Trees of supports (owner, 2026-10-30): two trunks merging into one; a support leaning
    # into the shaft beside it.
    # The Manual tool (swapped with 'supports' by the owner, 2026-10-30): a support with a plus
    # beside it; places the Automatic tool left unsupported,
    # an underside over two supports with a gap marked between them.
    'automatic': [
        closed([(9, 4), (15, 4), (12, 9)]), line((12, 9), (12, 19)), line((7, 20.5), (17, 20.5)),
        line((19.5, 2.5), (19.5, 7.5)), line((17, 5), (22, 5)),
    ],
    'left-unsupported': [
        line((3, 5), (21, 5)), line((5, 5), (5, 20)), line((19, 5), (19, 20)),
        line((12, 9), (12, 14)), line((12, 18), (12, 18)),
    ],
    'merge-trunks': [
        line((6, 3), (6, 10)), line((18, 3), (18, 10)),
        [(6, 10), (12, 15), (18, 10)], line((12, 15), (12, 20)), line((8, 21), (16, 21)),
    ],
    # Supports on a grid (TD-138): a flat underside seen from below, its tips in rows.
    'grid-supports': [
        closed([(3, 3), (21, 3), (21, 21), (3, 21)]),
        line((8, 8), (8, 8)), line((12, 8), (12, 8)), line((16, 8), (16, 8)),
        line((8, 12), (8, 12)), line((12, 12), (12, 12)), line((16, 12), (16, 12)),
        line((8, 16), (8, 16)), line((12, 16), (12, 16)), line((16, 16), (16, 16)),
    ],
    'join-support': [
        line((16, 3), (16, 21)), line((12, 21), (20, 21)),
        [(7, 3), (7, 11), (16, 17)],
    ],
    # A tool's list window (owner, 2026-10-29): rows, each with its dot.
    'list': [
        line((9, 6), (20, 6)), line((9, 12), (20, 12)), line((9, 18), (20, 18)),
        line((4.5, 6), (4.5, 6)), line((4.5, 12), (4.5, 12)), line((4.5, 18), (4.5, 18)),
    ],
    # The cavity view: the outside as a ghost (dashed) around the cavity, seen.
    'cavity-view': [arc((12, 12), 9, a, a + 25) for a in range(0, 360, 45)]
    + [arc((12, 12), 4.5, 0, 360)],
    'avoid-model': [
        arc((8, 21), 5, 180, 360),
        [(15, 3), (15, 7), (19, 11), (19, 21)],
    ],
    'knee': [
        [(6, 3.5), (13, 10), (13, 19)],
        line((9.5, 20.5), (16.5, 20.5)),
        closed([(13, 7.5), (15.5, 10), (13, 12.5), (10.5, 10)]),
    ],
    'overhangs': [
        [(4, 21), (4, 6), (20, 6), (20, 11), (10, 11), (10, 21)],
        line((12, 14), (14, 16)), line((15, 14), (17, 16)), line((18, 14), (20, 16)),
    ],
    # Keeping values into what exists (a support preset updated, TD-108): an arrow into a tray.
    'save': [
        [(4, 14), (4, 20), (20, 20), (20, 14)],
        line((12, 3.5), (12, 15)), [(7.5, 10.5), (12, 15), (16.5, 10.5)],
    ],
    'delete': [
        line((3.5, 6.5), (20.5, 6.5)), [(9, 6.5), (9, 3.5), (15, 3.5), (15, 6.5)],
        [(5.5, 6.5), (7, 20.5), (17, 20.5), (18.5, 6.5)],
        line((10, 10.5), (10, 16.5)), line((14, 10.5), (14, 16.5)),
    ],
    # Moving through layers (M6.5): a chevron each way, and to the first or last with a bar.
    'chevron-left': [[(15, 5), (8, 12), (15, 19)]],
    'chevron-right': [[(9, 5), (16, 12), (9, 19)]],
    # Folding the plates bar and opening it again (TD-135).
    'chevron-up': [[(5, 15), (12, 8), (19, 15)]],
    'chevron-down': [[(5, 9), (12, 16), (19, 9)]],
    'first': [line((6.5, 5), (6.5, 19)), [(17, 5), (10, 12), (17, 19)]],
    'last': [line((17.5, 5), (17.5, 19)), [(7, 5), (14, 12), (7, 19)]],
    # Zooming, closing a window or a notification, and removing an entry from a list.
    'plus': [line((12, 5), (12, 19)), line((5, 12), (19, 12))],
    'minus': [line((5, 12), (19, 12))],
    'close': [line((6.5, 6.5), (17.5, 17.5)), line((17.5, 6.5), (6.5, 17.5))],
    # Removing an entry from a list: a ring and a bar, the bar two strokes thick so it reads
    # at the small sizes rows show it at (owner, 2026-10-29).
    'remove': [
        arc((12, 12), 9.25, 0, 360),
        line((7.25, 11.3), (16.75, 11.3)), line((7.25, 12.7), (16.75, 12.7)),
    ],
    # Showing and hiding a model (owner, 2026-10-26): an eye, open or struck through. The lids
    # are arcs of radius 9.75 through the eye's corners at (3, 12) and (21, 12).
    'eye': [
        arc((12, 15.75), 9.75, 202.6, 337.4), arc((12, 8.25), 9.75, 22.6, 157.4),
        arc((12, 12), 3, 0, 360),
    ],
    'eye-off': [
        arc((12, 15.75), 9.75, 202.6, 337.4), arc((12, 8.25), 9.75, 22.6, 157.4),
        arc((12, 12), 3, 0, 360), line((4, 4), (20, 20)),
    ],
    'slice': [
        closed([(12, 3), (21, 8), (12, 13), (3, 8)]),
        [(3, 12.5), (12, 17.5), (21, 12.5)],
        [(3, 17), (12, 22), (21, 17)],
    ],
}


def segments(strokes):
    for points in strokes:
        for a, b in zip(points, points[1:]):
            yield a, b


def distance(p, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    length = dx * dx + dy * dy
    t = 0.0 if length == 0 else max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / length))
    return math.hypot(p[0] - ax - t * dx, p[1] - ay - t * dy)


def raster(strokes):
    scale = SIZE / GRID
    half = STROKE * scale / 2.0
    lines = [((a[0] * scale, a[1] * scale), (b[0] * scale, b[1] * scale)) for a, b in segments(strokes)]
    rows = []
    for y in range(SIZE):
        row = bytearray()
        for x in range(SIZE):
            centre = (x + 0.5, y + 0.5)
            nearest = min(distance(centre, a, b) for a, b in lines)
            alpha = max(0.0, min(1.0, half + 0.5 - nearest))
            row += bytes((255, 255, 255, int(round(alpha * 255))))
        rows.append(bytes(row))
    return rows


def png(width, height, rows):
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    raw = b''.join(b'\x00' + row for row in rows)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


def svg(strokes):
    def number(v):
        return ('%.3f' % v).rstrip('0').rstrip('.')
    paths = []
    for points in strokes:
        d = 'M' + ' L'.join('%s %s' % (number(x), number(y)) for x, y in points)
        paths.append('  <path d="%s"/>' % d)
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ffffff" '
            'stroke-width="%s" stroke-linecap="round" stroke-linejoin="round">\n%s\n</svg>\n'
            % (number(STROKE), '\n'.join(paths)))


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    folder = sys.argv[1]
    os.makedirs(folder, exist_ok=True)
    for name, strokes in sorted(ICONS.items()):
        with open(os.path.join(folder, name + '.png'), 'wb') as f:
            f.write(png(SIZE, SIZE, raster(strokes)))
        with open(os.path.join(folder, name + '.svg'), 'w', encoding='ascii') as f:
            f.write(svg(strokes))


if __name__ == '__main__':
    main()
