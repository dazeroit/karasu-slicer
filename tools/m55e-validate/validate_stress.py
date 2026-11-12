#!/usr/bin/env python3
"""Independent exact pixel oracle for the bounded eight-support fixture, not general topology."""
import hashlib
import json
from fractions import Fraction as F
from pathlib import Path
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate(directory):
    root = Path(directory)
    report = json.loads((root/'report.json').read_text())
    require(report['fixture'] == 'eight-separated-supports-v1', 'fixture')
    require(report['support_count'] == 8 and report['component_count'] == 24, 'counts')
    record = bytes(report['plan_record'])
    require(record[:5] == b'KSUP1' and record[13:19] == b'KPREP1', 'plan versions')
    source_digest = hashlib.sha256(Path(__file__).with_name('source.stl').read_bytes()).digest()
    require(record[19:51] == source_digest, 'upstream source identity')
    require(report['planes'] == [{'z_mm': 0.5, 'material_pixels': 3200},
                                 {'z_mm': 11.0, 'material_pixels': 39684}], 'planes')
    centers = [(x, y) for x in (9, 12, 15, 18) for y in (3, 9)]
    results = {}
    for name, base, expected_count in [('z0_5.pgm', True, 3200), ('z11.pgm', False, 39684)]:
        data = (root/name).read_bytes()
        header = b'P5\n200 200\n255\n'
        require(data.startswith(header) and len(data) == len(header)+40000, 'PGM framing')
        pixels = data[len(header):]
        for index, value in enumerate(pixels):
            require(value in (0, 255), 'binary intensity')
            px, py = F(2*(index % 200)+1, 20), F(2*(index // 200)+1, 20)
            if base:
                filled = any(x-1 <= px < x+1 and y-1 <= py < y+1 for x, y in centers)
            else:
                squared = (px-5)**2+(py-5)**2
                require(not F(999, 1000)**2 <= squared <= F(1001, 1000)**2,
                        'owned fixture sample enters radial uncertainty band')
                filled = squared > 1
            require(value == (255 if filled else 0), f'{name}: pixel {index}')
        require(sum(value != 0 for value in pixels) == expected_count, 'pixel count')
        results[name] = {'checked_pixels': len(pixels), 'material_pixels': expected_count,
                         'sha256': hashlib.sha256(data).hexdigest()}
    return {'status': 'passed', 'masks': results,
            'plan_sha256': hashlib.sha256(record).hexdigest()}


if __name__ == '__main__':
    print(json.dumps(validate(Path(sys.argv[1])), sort_keys=True))
