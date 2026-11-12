#!/usr/bin/env python3
"""Independent exact mesh checks and analytic combined-preparation mask oracle.

This validates one owned hollowed, drained and rectangular-supported fixture, not arbitrary contacts,
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


def expected_pixel(x, y, z, radial_error=F(1, 1000)):
    """Analytic fixture oracle; None denotes only the certified radial annulus.

    Subtracted cavity boundary is absent at its bottom cap (z=12), matching
    closed negative-solid membership. Exterior/platform caps are present.
    """
    px, py = F(2*x+1, 20), F(2*y+1, 20)
    if z >= 10:
        if 12 <= z <= 28 and 2 <= px <= 18 and 2 <= py <= 18:
            return 0
        if 9 <= z <= 13:
            radius_squared = (px-5)**2 + (py-5)**2
            if radius_squared < (1-radial_error)**2:
                return 0
            if radius_squared <= (1+radial_error)**2:
                return None
        return 255
    if z < 0:
        return 0
    if z < 1:
        width = F(4)
    elif z < 9:
        width = 4-F(3)*(z-1)/8
    else:
        width = 1-F(6, 10)*(z-9)/F(13, 10)
    return 255 if 12-width/2 <= px < 12+width/2 and 8-width/2 <= py < 8+width/2 else 0


def check_mask(pixels, z, radial_error):
    require(len(pixels) == 40000, 'mask length')
    require(all(v in (0, 255) for v in pixels), 'nonbinary mask')
    checked = uncertain = 0
    for i, value in enumerate(pixels):
        expected = expected_pixel(i % 200, i // 200, z, radial_error)
        if expected is None:
            uncertain += 1
        else:
            checked += 1
            require(value == expected, f'analytic mask mismatch z={z}, pixel={i}')
    return checked, uncertain


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
    require(report['fixture'] == geometry['fixture'] == 'cube20-z10-hollow-drained-supported-v1', 'fixture identity')
    require(report['panel_dimensions'] == [200, 200] and report['panel_extent_mm'] == [20., 20.], 'panel')
    require(report['samples_per_axis'] == 1 and report['grayscale_levels'] == 2, 'raster mode')
    source_bytes = (root/'source.stl').read_bytes()
    source_hash = hashlib.sha256(source_bytes).hexdigest()
    require(source_hash == report['source_sha256'] == report['model_binding_sha256'] == 'f8fe73215c28859baff91db874d51fbbe0a6c62ef206a80eb5923df7bec8bbc6', 'owned source-byte identity')
    vertices, triangles = [], []
    for line in source_bytes.decode('ascii').splitlines():
        words = line.split()
        if words and words[0] == 'vertex':
            point = [float(v) for v in words[1:]]
            if point not in vertices:
                vertices.append(point)
            if not triangles or len(triangles[-1]) == 3:
                triangles.append([])
            triangles[-1].append(vertices.index(point))
    require(geometry['source'] == {'vertices': vertices, 'triangles': triangles}, 'imported source differs from owned STL')
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
        lower_z, upper_z = sorted({v[2] for v in expected})
        widths = []
        for z in [lower_z, upper_z]:
            xs = [p[0] for p in expected if p[2] == z]
            ys = [p[1] for p in expected if p[2] == z]
            widths.append((max(xs)-min(xs), max(ys)-min(ys)))
        (w, h), (u, v) = widths
        dw, dh = u-w, v-h
        require(volumes[index] == (upper_z-lower_z)*(w*h+(w*dh+h*dw)/2+dw*dh/3), 'exact analytic loft volume')
    cert = report['certificates']
    require(len(cert) == 1 and cert[0]['support_id'] == 1, 'certificate identity')
    require(0 < F(cert[0]['minimum_penetration_mm']) <= F(10.3)-10 <= F(cert[0]['maximum_penetration_mm']) <= F(1, 2), 'penetration interval')
    require(report['enclosed_cavities'] == 0, 'drain connectivity')
    radial_error = F(report['radial_error_mm'])
    require(0 <= radial_error <= F(1, 1000), 'radial error budget')
    shell, cavity = geometry['shell'], geometry['cavity']
    require(validate_mesh(cavity) == 4096, 'cavity volume')
    require(shell['vertices'][:8] == [[x, y, z+10] for x, y, z in geometry['source']['vertices']], 'placed exterior changed')
    require(shell['triangles'][:12] == geometry['source']['triangles'], 'source triangles changed')
    require(shell['vertices'][8:] == cavity['vertices'], 'cavity append changed')
    require(shell['triangles'][12:] == [[a+8, c+8, b+8] for a, b, c in cavity['triangles']], 'cavity orientation/index changed')
    planes = dict(zip(['z0.pgm','z0_5.pgm','z1.pgm','z5.pgm','z9.pgm','z9_5.pgm','z10.pgm','z11.pgm','z12.pgm','z12_5.pgm','z13.pgm','z14.pgm'], map(F, [0,.5,1,5,9,9.5,10,11,12,12.5,13,14])))
    masks = {}
    checked_pixels = radial_band_pixels = 0
    require({p['file']: F(p['z_mm']) for p in report['planes']} == planes, 'plane fixture')
    for name, z in planes.items():
        data = (root/name).read_bytes()
        header = b'P5\n200 200\n255\n'
        require(data.startswith(header) and len(data) == len(header)+40000, 'PGM shape')
        pixels = data[len(header):]
        checked, band = check_mask(pixels, z, radial_error)
        checked_pixels += checked
        radial_band_pixels += band
        masks[name] = hashlib.sha256(pixels).hexdigest()
    formats = {}
    reference_checked_pixels = reference_radial_band_pixels = 0
    if machine:
        reference = load_tool('m5-validate', 'validate_reference.py', 'support_reference_reader')
        reference.validate(root/'reference')
        sys.modules['validate_reference'] = reference
        png = reference.PNG
        hashes = []
        for i, z in enumerate([F(i, 2) for i in range(1, 29)]):
            w, h, pixels = png.decode((root/'reference'/f'layer-{i:08}.png').read_bytes())
            require((w, h) == (200, 200), 'reference panel')
            checked, band = check_mask(pixels, z, radial_error)
            reference_checked_pixels += checked
            reference_radial_band_pixels += band
            hashes.append(hashlib.sha256(pixels).hexdigest())
        reader = load_tool('m5-validate', 'validate_machine.py', 'support_machine_reader')
        for name in ['preparation.ctb', 'preparation.goo', 'preparation.photon']:
            decoded = reader.validate(root/name)
            require([layer['mask_sha256'] for layer in decoded['layers']] == hashes, f'machine mask mismatch: {name}')
            formats[name] = decoded['format']
        pp1 = load_tool('m5-validate', 'validate_pp1.py', 'support_pp1_reader')
        decoded = pp1.validate(root/'preparation.pp1', root/'reference')
        formats['preparation.pp1'] = 'anycubic-pp1-v3'
    return {'fixture': report['fixture'], 'components': len(components), 'component_volume_mm3': [float(v) for v in volumes], 'checked_pixels': checked_pixels, 'radial_band_pixels': radial_band_pixels, 'reference_checked_pixels': reference_checked_pixels, 'reference_radial_band_pixels': reference_radial_band_pixels, 'mask_sha256': masks, 'machine_formats': formats}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory')
    parser.add_argument('--without-machine', action='store_true')
    args = parser.parse_args()
    print(json.dumps(validate(args.directory, not args.without_machine), sort_keys=True))
