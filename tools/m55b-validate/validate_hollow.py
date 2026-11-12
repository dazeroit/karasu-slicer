#!/usr/bin/env python3
"""Independent exact-rational checks of represented hollow_reference output.

This reader does not implement clipping or hull reconstruction and imports no Karasu
code. Passing validates the stated checks for these files, not arbitrary topology,
the reverse erosion inclusion, source SHA assertions, drainage, or printability.
"""
import argparse
from collections import defaultdict
from fractions import Fraction
import hashlib
import json
import math
from pathlib import Path
import sys


class ValidationError(ValueError):
    """A recorded geometry or certificate invariant was violated."""


def require(condition, message):
    if not condition:
        raise ValidationError(message)


def number(value):
    require(type(value) in (int, float), "expected a real JSON number")
    require(math.isfinite(value), "nonfinite number")
    return Fraction(value)


def load_document(path):
    data = Path(path).read_bytes()
    require(len(data) <= 64 * 1024 * 1024, "JSON document exceeds reader limit")
    def reject_constant(value):
        raise ValidationError("nonstandard JSON number: " + value)
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    return json.loads(data, parse_constant=reject_constant, object_pairs_hook=unique), hashlib.sha256(data).hexdigest()


def subtract(left, right):
    return tuple(a-b for a, b in zip(left, right))


def cross(left, right):
    return (left[1]*right[2]-left[2]*right[1],
            left[2]*right[0]-left[0]*right[2],
            left[0]*right[1]-left[1]*right[0])


def dot(left, right):
    return sum(a*b for a, b in zip(left, right))


def mesh(document):
    require(isinstance(document, dict), "mesh must be an object")
    points, triangles = document["vertices"], document["triangles"]
    require(isinstance(points, list) and 4 <= len(points) <= 100_000, "vertex count")
    require(isinstance(triangles, list) and 4 <= len(triangles) <= 100_000, "triangle count")
    vertices = []
    for point in points:
        require(isinstance(point, list) and len(point) == 3, "vertex dimensions")
        vertices.append(tuple(number(value) for value in point))
    require(len(set(vertices)) == len(vertices), "duplicate vertex coordinates")
    faces = []
    for triangle in triangles:
        require(isinstance(triangle, list) and len(triangle) == 3, "triangle dimensions")
        require(all(type(index) is int and 0 <= index < len(vertices) for index in triangle), "triangle index")
        require(len(set(triangle)) == 3, "repeated triangle index")
        faces.append(tuple(triangle))
    return vertices, faces


def topology(vertices, faces):
    edges = defaultdict(list)
    used = set()
    for index, face in enumerate(faces):
        used.update(face)
        a, b, c = (vertices[v] for v in face)
        require(any(cross(subtract(b, a), subtract(c, a))), "zero-area triangle")
        for left, right in zip(face, face[1:]+face[:1]):
            edges[min(left, right), max(left, right)].append((index, left, right))
    require(len(used) == len(vertices), "isolated vertices")
    adjacency = [[] for _ in faces]
    for incidents in edges.values():
        require(len(incidents) == 2, "edge must have exactly two incident faces")
        first, second = incidents
        require(first[1:] == (second[2], second[1]), "edge winding must oppose")
        adjacency[first[0]].append(second[0])
        adjacency[second[0]].append(first[0])
    unseen = set(range(len(faces)))
    components = 0
    while unseen:
        components += 1
        pending = [unseen.pop()]
        while pending:
            for neighbor in adjacency[pending.pop()]:
                if neighbor in unseen:
                    unseen.remove(neighbor)
                    pending.append(neighbor)
    return components


def volume(vertices, faces):
    return sum((dot(vertices[a], cross(vertices[b], vertices[c])) for a, b, c in faces), Fraction())/6


def convex_supports(vertices, faces):
    supports = []
    for face in faces:
        a, b, c = (vertices[index] for index in face)
        normal = cross(subtract(b, a), subtract(c, a))
        sides = [dot(normal, subtract(point, a)) for point in vertices]
        require(all(side <= 0 for side in sides) and any(side < 0 for side in sides),
                "triangle is not an outward supporting plane")
        supports.append((a, normal))
    return supports


def bounded(report, key, exact):
    interval = report[key]
    require(isinstance(interval, list) and len(interval) == 2, "volume interval dimensions")
    low, high = map(number, interval)
    require(0 <= low <= exact <= high, "exact volume not enclosed: " + key)


def validate_documents(geometry, report, thickness=2, cube=False):
    require(isinstance(geometry, dict) and isinstance(report, dict), "documents must be objects")
    require(type(geometry.get("schema")) is int and geometry["schema"] == 1, "geometry schema")
    require(report.get("backend") == "convex-halfspaces-v1", "backend identity")
    source, cavity, shell = (mesh(geometry[key]) for key in ("source", "cavity", "shell"))
    for (points, faces), expected in zip((source, cavity, shell), (1, 1, 2)):
        require(topology(points, faces) == expected, "unexpected face-component count")
    source_supports = convex_supports(*source)
    convex_supports(*cavity)
    for key, expected in (("vertices", len(shell[0])), ("triangles", len(shell[1]))):
        require(type(report.get(key)) is int and report[key] == expected, "reported mesh count: " + key)
    source_points, source_faces = source
    cavity_points, cavity_faces = cavity
    shell_points, shell_faces = shell
    require(shell_points == source_points+cavity_points, "exterior/cavity vertex preservation")
    offset = len(source_points)
    expected_faces = source_faces+[(a+offset, c+offset, b+offset) for a, b, c in cavity_faces]
    require(shell_faces == expected_faces, "exterior/reversed cavity face preservation")
    source_volume, cavity_volume, material_volume = (volume(*item) for item in (source, cavity, shell))
    require(source_volume > cavity_volume > 0, "cavity volume must be positive and less than source")
    require(material_volume == source_volume-cavity_volume, "shell material volume identity")
    bounded(report, "source_volume_mm3", source_volume)
    bounded(report, "cavity_volume_mm3", cavity_volume)
    bounded(report, "material_volume_mm3", material_volume)
    delta, requested = number(report["wall_offset_error_mm"]), number(thickness)
    require(0 <= delta < requested, "offset error must be nonnegative and less than thickness")
    distance = requested-delta
    for anchor, normal in source_supports:
        squared_requirement = distance*distance*dot(normal, normal)
        for point in cavity_points:
            side = dot(normal, subtract(point, anchor))
            require(side < 0 and side*side >= squared_requirement,
                    "cavity vertex violates certified inward offset")
    if cube:
        require(source_volume == 8000, "cube fixture source volume")
        bounded(report, "cavity_volume_mm3", Fraction(4096))
        bounded(report, "material_volume_mm3", Fraction(3904))
        for axis in range(3):
            require(min(point[axis] for point in source_points) == 0 and
                    max(point[axis] for point in source_points) == 20, "cube source extent")
            require(abs(min(point[axis] for point in cavity_points)-2) <= delta and
                    abs(max(point[axis] for point in cavity_points)-18) <= delta,
                    "cube cavity extent")
    return {"validated": True, "source_components": 1, "cavity_components": 1,
            "shell_components": 2, "source_volume_mm3": float(source_volume),
            "cavity_volume_mm3": float(cavity_volume), "material_volume_mm3": float(material_volume),
            "offset_inequality_pairs": len(source_supports)*len(cavity_points)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--cube", action="store_true")
    parser.add_argument("--thickness", type=float, default=2.)
    args = parser.parse_args()
    try:
        geometry, geometry_hash = load_document(args.directory/"geometry.json")
        report, report_hash = load_document(args.directory/"report.json")
        result = validate_documents(geometry, report, args.thickness, args.cube)
        result.update(geometry_sha256=geometry_hash, report_sha256=report_hash)
        print(json.dumps(result, sort_keys=True))
    except (OSError, ValueError, KeyError, TypeError, OverflowError, RecursionError) as error:
        print("hollow validation failed: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
