#!/usr/bin/env python3
"""Independent exact mesh checks and analytic mask oracle for support_reference.

This validates one owned rectangular fixture, not arbitrary support contacts,
vertex links, coplanar contacts, physical printability or commercial compatibility.
"""
import argparse
from collections import Counter
from fractions import Fraction as F
import hashlib
import importlib.util
import json
import sys
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def vector(a, b):
    return [a[i]-b[i] for i in range(3)]


def cross(a, b):
    return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def validate_mesh(mesh):
    vertices = [[F(v) for v in p] for p in mesh['vertices']]
    triangles = mesh['triangles']
    require(vertices and triangles, 'empty mesh')
    require(len(set(map(tuple, vertices))) == len(vertices), 'duplicate vertices')
    edges = Counter()
    volume6 = F(0)
    neighbors = {i: set() for i in range(len(vertices))}
    for face in triangles:
        require(len(face) == len(set(face)) == 3, 'invalid triangle')
        require(all(isinstance(i, int) and 0 <= i < len(vertices) for i in face), 'index range')
        a, b, c = (vertices[i] for i in face)
        normal = cross(vector(b, a), vector(c, a))
        require(any(normal), 'degenerate face')
        require(all(dot(normal, vector(p, a)) <= 0 for p in vertices), 'nonconvex/inward face')
        volume6 += dot(a, cross(b, c))
        for u, v in zip(face, face[1:]+face[:1]):
            edges[u, v] += 1
            neighbors[u].add(v)
            neighbors[v].add(u)
    require(all(n == 1 and edges[v, u] == 1 for (u, v), n in edges.items()), 'open/nonmanifold edge')
    seen, pending = set(), [0]
    while pending:
        point = pending.pop()
        if point not in seen:
            seen.add(point)
            pending.extend(neighbors[point]-seen)
    require(len(seen) == len(vertices), 'disconnected/unused vertices')
    require(volume6 > 0, 'nonpositive volume')
    return volume6/6


def expected_pixel(x, y, z):
    px, py = F(2*x+1, 20), F(2*y+1, 20)
    if z >= 10:
        return 0 if 12 < z < 28 and 2 < px < 18 and 2 < py < 18 else 255
    if z < 1:
        width = F(4)
    elif z < 9:
        width = 4-F(3)*(z-1)/8
    else:
        width = 1-F(6, 10)*(z-9)/F(13, 10)
    # Documented half-open polygon coverage at an exact sample/edge coincidence.
    return 255 if 12-width/2 <= px < 12+width/2 and 8-width/2 <= py < 8+width/2 else 0


def load_tool(directory, filename, name):
    path = Path(__file__).resolve().parents[1]/directory/filename
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def validate(directory, machine=True):
    root = Path(directory)
    report = json.loads((root/'report.json').read_text())
    geometry = json.loads((root/'geometry.json').read_text())
    require(report['schema'] == geometry['schema'] == 1, 'schema')
    require(report['fixture'] == geometry['fixture'] == 'cube20-z10-rectangular-support-v1', 'fixture identity')
    require(report['panel_dimensions'] == [200, 200] and report['panel_extent_mm'] == [20., 20.], 'panel')
    require(report['samples_per_axis'] == 1 and report['grayscale_levels'] == 2, 'raster mode')
    source_volume = validate_mesh(geometry['source'])
    require(source_volume == 8000, 'source volume')
    require({tuple(p) for p in geometry['source']['vertices']} == {(x, y, z) for x in [0, 20] for y in [0, 20] for z in [0, 20]}, 'owned source cube corners')
    components = geometry['components']
    require(len(components) == 3, 'component count')
    volumes = [validate_mesh(mesh) for mesh in components]
    stations = geometry['stations']
    require([(s['center_mm'], s['rectangle_full_size_mm']) for s in stations] == [([12., 8., 0.], [4., 4.]), ([12., 8., 1.], [4., 4.]), ([12., 8., 9.], [1., 1.]), ([12., 8., 10.3], [.4, .4])], 'authored fixture parameters')
    for index, mesh in enumerate(components):
        expected = set()
        for s in stations[index:index+2]:
            x, y, z = map(F, s['center_mm'])
            w, h = map(F, s['rectangle_full_size_mm'])
            # Reproduce only authored IEEE station-point arithmetic, not the Rust hull.
            for sx, sy in [(-1, -1), (1, -1), (1, 1), (-1, 1)]:
                expected.add((F(float(x)+sx*float(w)/2), F(float(y)+sy*float(h)/2), z))
        require(set(tuple(F(v) for v in p) for p in mesh['vertices']) == expected, 'component does not preserve station corners')
    cert = report['certificates']
    require(len(cert) == 1 and cert[0]['support_id'] == 1, 'certificate identity')
    require(0 < F(cert[0]['minimum_penetration_mm']) <= F(10.3)-10 <= F(cert[0]['maximum_penetration_mm']) <= F(1, 2), 'penetration interval')
    planes = {'z0_5.pgm': F(1, 2), 'z5.pgm': F(5), 'z9_5.pgm': F(19, 2), 'z11.pgm': F(11), 'z13.pgm': F(13)}
    masks = {}
    require({p['file']: F(p['z_mm']) for p in report['planes']} == planes, 'plane fixture')
    for name, z in planes.items():
        data = (root/name).read_bytes()
        header = b'P5\n200 200\n255\n'
        require(data.startswith(header) and len(data) == len(header)+40000, 'PGM shape')
        pixels = data[len(header):]
        require(all(pixel == expected_pixel(i % 200, i // 200, z) for i, pixel in enumerate(pixels)), f'analytic mask mismatch: {name}')
        masks[name] = hashlib.sha256(pixels).hexdigest()
    formats = {}
    if machine:
        reference = load_tool('m5-validate', 'validate_reference.py', 'support_reference_reader')
        reference.validate(root/'reference')
        sys.modules['validate_reference'] = reference
        png = reference.PNG
        hashes = []
        for i, z in enumerate([F(1, 2), F(5), F(19, 2), F(14), F(37, 2)]):
            w, h, pixels = png.decode((root/'reference'/f'layer-{i:08}.png').read_bytes())
            require((w, h) == (200, 200), 'reference panel')
            require(all(v == expected_pixel(j % 200, j // 200, z) for j, v in enumerate(pixels)), f'exported layer mask mismatch: {i}')
            hashes.append(hashlib.sha256(pixels).hexdigest())
        reader = load_tool('m5-validate', 'validate_machine.py', 'support_machine_reader')
        for name in ['support.ctb', 'support.goo', 'support.photon']:
            decoded = reader.validate(root/name)
            require([layer['mask_sha256'] for layer in decoded['layers']] == hashes, f'machine mask mismatch: {name}')
            formats[name] = decoded['format']
        pp1 = load_tool('m5-validate', 'validate_pp1.py', 'support_pp1_reader')
        decoded = pp1.validate(root/'support.pp1', root/'reference')
        formats['support.pp1'] = 'anycubic-pp1-v3'
    return {'fixture': report['fixture'], 'components': len(components), 'component_volume_mm3': [float(v) for v in volumes], 'checked_pixels': 200000, 'reference_checked_pixels': 200000 if machine else 0, 'mask_sha256': masks, 'machine_formats': formats}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory')
    parser.add_argument('--without-machine', action='store_true')
    args = parser.parse_args()
    print(json.dumps(validate(args.directory, not args.without_machine), sort_keys=True))
