"""Compare the panel orientation of printer files written for the same model and printer.

usage: compare.py REFERENCE CANDIDATE [LAYER]
       compare.py --dirs REFERENCE_DIR CANDIDATE_DIR

One layer of each file (by default the middle one) becomes a silhouette: cells of a few pixels,
each row filled between its first and last lit cell, so that a hollow model's thin walls and
anti-aliased edges compare by outline. The candidate's silhouette, as stored, mirrored along
columns (X), mirrored along rows (Y) and turned 180 degrees, is compared with the reference's by
intersection over union, in place on the panel and with both centred on their centroids (slicers
may place the model a little differently); each flip scores the better of the two. The verdict
names the flip that stands out. The files' own declarations (GOO mirror bytes, PP1 `rotate_z`)
are printed beside it.

When the candidate's panel is the reference's with its sides exchanged (2560 x 1440 against
1440 x 2560), two readings are tried. The part may keep its orientation in the image while the
panel's sides are exchanged: the slicers then disagree on which plate side is X, and only the
centred comparison applies ("exchanged panel"); in karasu-slicer's printer profile that is the
resolution and the plate's X and Y sizes exchanged. Or the image may be transposed: transposing
alone mirrors the part across the diagonal, transposing and then mirroring along one axis turns it
a quarter turn; in the profile that is `swap_xy`, then `mirror_x`/`mirror_y`.

With `--dirs`, every reference file is paired with the candidates of the same extension named
after it: the same stem (the printer id), or the stem followed by `--` and a format id, as
`export_karasu.py` names a printer's second format. One Markdown row is printed per pair.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from print_files import read  # noqa: E402

FLIPS = ('as stored', 'mirror X', 'mirror Y', 'turned 180')
MEANING = {'as stored': 'same image', 'turned 180': 'turned 180 degrees, same handedness',
           'mirror X': 'MIRRORED', 'mirror Y': 'MIRRORED'}
EXCHANGED_MEANING = {'as stored': 'same image on an exchanged panel',
                     'turned 180': 'turned 180 degrees on an exchanged panel, same handedness',
                     'mirror X': 'MIRRORED (exchanged panel)',
                     'mirror Y': 'MIRRORED (exchanged panel)'}
TRANSPOSED_MEANING = {'as stored': 'MIRRORED (transposed)',
                      'turned 180': 'MIRRORED (transposed, turned 180 degrees)',
                      'mirror X': 'turned 90 degrees (transposed, mirror X), same handedness',
                      'mirror Y': 'turned 90 degrees (transposed, mirror Y), same handedness'}


def transpose(pixels, width, height):
    """The image with rows and columns exchanged: `height` columns by `width` rows."""
    return b''.join(pixels[x::width] for x in range(width))


def silhouette(pixels, width, height, size):
    """Cells of `size` x `size` pixels, every `size`-th row sampled and filled between its first
    and last lit pixel."""
    out = set()
    zero = b'\0'
    for y in range(0, height, size):
        row = pixels[y * width:(y + 1) * width]
        if not row.strip(zero):
            continue
        first = (len(row) - len(row.lstrip(zero))) // size
        last = (len(row.rstrip(zero)) - 1) // size
        out.update((x, y // size) for x in range(first, last + 1))
    return out


def flip(points, kind, cols, rows):
    if kind == 'mirror X':
        return {(cols - 1 - x, y) for x, y in points}
    if kind == 'mirror Y':
        return {(x, rows - 1 - y) for x, y in points}
    if kind == 'turned 180':
        return {(cols - 1 - x, rows - 1 - y) for x, y in points}
    return set(points)


def centred(points):
    if not points:
        return set()
    cx = round(sum(x for x, _ in points) / len(points))
    cy = round(sum(y for _, y in points) / len(points))
    return {(x - cx, y - cy) for x, y in points}


def iou(a, b):
    return len(a & b) / max(1, len(a | b))


def compare(reference_path, candidate_path, layer=None):
    ref, can = read(reference_path), read(candidate_path)
    transposed = (ref.width, ref.height) == (can.height, can.width) != (can.width, can.height)
    if (ref.width, ref.height) != (can.width, can.height) and not transposed:
        raise ValueError(f'panels differ: {ref.width}x{ref.height} and {can.width}x{can.height}')
    size = max(1, round(max(ref.width, ref.height) / 2048))
    cols, rows = -(-ref.width // size), -(-ref.height // size)
    a = silhouette(ref.layer(ref.count // 2 if layer is None else layer), ref.width, ref.height,
                   size)
    pixels = can.layer(can.count // 2 if layer is None else layer)
    if not transposed:
        return {'reference': ref, 'candidate': can, 'mode': 'same panel',
                **judge(a, silhouette(pixels, ref.width, ref.height, size), cols, rows)}
    turned = judge(a, silhouette(transpose(pixels, can.width, can.height), ref.width, ref.height,
                                 size), cols, rows)
    exchanged = judge(a, silhouette(pixels, can.width, can.height, size),
                      -(-can.width // size), -(-can.height // size), in_place=False)
    mode, result = max((('exchanged panel', exchanged), ('transposed', turned)),
                       key=lambda m: (m[1]['verdict'] is not None, max(m[1]['score'].values())))
    return {'reference': ref, 'candidate': can, 'mode': mode, **result}


def judge(a, b, cols, rows, in_place=True):
    """Scores of silhouette `b` (on a `cols` x `rows` grid) under each flip against `a`, and the
    flip that stands out (at least 0.85, and 0.2 above the next), or None. Without `in_place`,
    for panels of different shapes, only the centred comparison counts."""
    in_place = {k: iou(a, flip(b, k, cols, rows)) if in_place else 0.0 for k in FLIPS}
    aligned = {k: iou(centred(a), centred(flip(b, k, cols, rows))) for k in FLIPS}
    score = {k: max(in_place[k], aligned[k]) for k in FLIPS}
    ranked = sorted(FLIPS, key=score.get, reverse=True)
    best, second = ranked[0], ranked[1]
    clear = score[best] >= 0.85 and score[best] - score[second] >= 0.2
    return {'in_place': in_place, 'aligned': aligned, 'score': score,
            'verdict': best if clear else None}


def describe(result):
    v = result['verdict']
    if not v:
        return 'unclear'
    return {'transposed': TRANSPOSED_MEANING, 'exchanged panel': EXCHANGED_MEANING}.get(
        result.get('mode'), MEANING)[v]


def main(argv):
    if len(argv) == 3 and argv[0] == '--dirs':
        print('| Reference | Candidate | Verdict | Score: as stored / mirror X / mirror Y / '
              'turned 180 | Declared (reference; candidate) |')
        print('| --- | --- | --- | --- | --- |')
        candidates = sorted(p for p in Path(argv[2]).iterdir() if p.is_file())
        for ref in sorted(p for p in Path(argv[1]).rglob('*') if p.is_file()):
            stem = ref.stem.lower()
            matches = [c for c in candidates if c.suffix.lower() == ref.suffix.lower()
                       and (c.stem.lower() == stem or c.stem.lower().startswith(stem + '--'))]
            if not matches:
                print(f'| {ref.name} | (none) | | | |')
                continue
            for can in matches:
                try:
                    r = compare(ref, can)
                except Exception as error:  # report the pair and go on with the others
                    print(f'| {ref.name} | {can.name} | error: {error} | | |')
                    continue
                scores = ' / '.join(f"{r['score'][k]:.2f}" for k in FLIPS)
                print(f"| {ref.name} | {can.name} | {describe(r)} | {scores} | "
                      f"{r['reference'].declared or '-'}; {r['candidate'].declared or '-'} |")
        return 0
    if len(argv) not in (2, 3):
        print(__doc__)
        return 2
    r = compare(argv[0], argv[1], int(argv[2]) if len(argv) == 3 else None)
    for name, f in (('reference', r['reference']), ('candidate', r['candidate'])):
        print(f'{name}: {f.path.name} ({f.software or "?"}), {f.width}x{f.height}, '
              f'{f.count} layers, declared {f.declared or "nothing"}')
    for k in FLIPS:
        print(f"  {k:10s}  score {r['score'][k]:.2f}  (in place {r['in_place'][k]:.2f}, "
              f"centred {r['aligned'][k]:.2f})")
    print('verdict:', describe(r))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
