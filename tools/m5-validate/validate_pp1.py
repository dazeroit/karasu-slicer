#!/usr/bin/env python3
"""Independent bounded PP1/PW0 reader; optional exact comparison with reference PNG masks."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import zipfile
import zlib

from validate_reference import PNG, require

MAX_PIXELS = 128 * 1024 * 1024
MAX_MEMBER = 256 * 1024 * 1024
MAX_METADATA = 16 * 1024 * 1024


def decode_pw0(data, count):
    """Decode strict finite runs; reject tolerated vendor truncation shortcuts."""
    require(0 < count <= MAX_PIXELS, "PW0 pixel budget")
    output = bytearray()
    at = 0
    while at < len(data):
        tag = data[at]
        at += 1
        color, run = tag >> 4, tag & 15
        if color in (0, 15):
            require(at < len(data), "truncated PW0 long run")
            run = run * 256 + data[at]
            at += 1
        require(run > 0, "zero PW0 run")
        require(len(output) + run <= count, "PW0 raster overrun")
        output.extend(bytes([color * 17]) * run)
    require(len(output) == count, "PW0 raster underrun")
    return output


def check_png(data):
    """Check ordinary eight-bit PNG chunk CRCs and bounded decompressed scanlines.

    Returns [width, height, channels]: 1 for gray projections, 3 for rendered RGB previews."""
    require(data[:8] == b"\x89PNG\r\n\x1a\n", "PNG signature")
    at = 8
    payload = bytearray()
    dimensions = None
    ended = False
    while at < len(data):
        require(at + 12 <= len(data), "truncated PNG chunk")
        size = struct.unpack_from(">I", data, at)[0]
        kind = data[at + 4:at + 8]
        require(at + 12 + size <= len(data), "PNG chunk length")
        body = data[at + 8:at + 8 + size]
        crc = struct.unpack_from(">I", data, at + 8 + size)[0]
        require(zlib.crc32(kind + body) == crc, "PNG chunk CRC")
        if kind == b"IHDR":
            require(dimensions is None and size == 13, "duplicate/invalid PNG IHDR")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", body)
            require(0 < width * height <= MAX_PIXELS and depth == 8, "PNG dimensions/depth")
            require(color in (0, 2, 4, 6) and compression == filtering == interlace == 0, "PNG encoding")
            channels = {0: 1, 2: 3, 4: 2, 6: 4}[color]
            dimensions = width, height, channels
        elif kind == b"IDAT":
            require(dimensions is not None and not ended, "PNG IDAT order")
            payload.extend(body)
        elif kind == b"IEND":
            require(size == 0, "PNG IEND data")
            ended = True
        at += size + 12
        if ended:
            require(at == len(data), "trailing PNG data")
    require(ended and dimensions is not None, "missing PNG end/header")
    width, height, channels = dimensions
    expected = height * (1 + width * channels)
    require(expected <= MAX_MEMBER, "PNG decompression budget")
    stream = zlib.decompressobj()
    raw = stream.decompress(payload, expected + 1)
    require(len(raw) == expected and stream.eof and not stream.unused_data, "PNG scanline/zlib length")
    require(all(raw[y * (1 + width * channels)] <= 4 for y in range(height)), "PNG filter type")
    return [width, height, channels]


def raster_components(pixels, width, height):
    """Independent flood fill for small validation fixtures, distinct from Rust's scanline union."""
    require(len(pixels) <= 100_000, "component fixture budget")
    seen = set()
    sizes = []
    for start, value in enumerate(pixels):
        if value == 0 or start in seen:
            continue
        seen.add(start)
        todo = [start]
        size = 0
        while todo:
            point = todo.pop()
            size += 1
            y, x = divmod(point, width)
            for yy in range(max(0, y - 1), min(height, y + 2)):
                for xx in range(max(0, x - 1), min(width, x + 2)):
                    neighbor = yy * width + xx
                    if pixels[neighbor] and neighbor not in seen:
                        seen.add(neighbor)
                        todo.append(neighbor)
        sizes.append(size)
    return len(sizes), max(sizes, default=0)


def validate(path, reference_dir=None, sample_layers=None, commercial=False):
    """Commercial mode checks layout, not equivalence to Karasu's independent scene-area meaning."""
    path = Path(path)
    require(path.stat().st_size <= 1024 * 1024 * 1024, "archive byte budget")
    with path.open("rb") as raw:
        digest = hashlib.file_digest(raw, "sha256").hexdigest()
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        require(len(names) == len(set(names)) <= 65535, "duplicate/excess ZIP members")
        require(all(not n.startswith("/") and "\\" not in n and all(p not in ("", ".", "..") for p in n.split("/")) for n in names), "ZIP paths")
        require(all(info.file_size <= MAX_MEMBER and not info.flag_bits & 1 for info in archive.infolist()), "ZIP member budget/encryption")

        def read(name, budget=MAX_MEMBER):
            require(archive.getinfo(name).file_size <= budget, "member budget")
            # ZipFile checks stored member CRC independently of the Rust writer.
            return archive.read(name)

        manifest = json.loads(read("anycubic_photon_resins.pwsp", MAX_METADATA))
        controller = json.loads(read("layers_controller.conf", MAX_METADATA))
        machine = manifest["machine_type"]
        require(machine["key_suffix"] == "pp1", "PP1 suffix")
        width, height = machine["res_x"], machine["res_y"]
        require(isinstance(width, int) and isinstance(height, int) and 0 < width <= 65536 and 0 < height <= 65536 and width * height <= MAX_PIXELS, "PP1 dimensions")
        count = controller["count"]
        require(isinstance(count, int) and 0 < count <= 65525 and len(controller["paras"]) == count, "PP1 layer count")
        require(set(n for n in names if n.startswith("layer_images/")) == {f"layer_images/layer_{i}.pw0Img" for i in range(count)}, "PW0 layer members")
        scene = read("scene.slice", MAX_METADATA)
        require(len(scene) == 400 + 64 * count and scene[:16] == b"ANYCUBIC-PWSZ\0\0\0", "scene layout")
        require(struct.unpack_from("<I", scene, 84)[0] == 3, "scene revision")
        require(struct.unpack_from("<I", scene, 100)[0] == count == struct.unpack_from("<I", scene, 392)[0], "scene count")
        require(scene[388:392] == b"<---" and scene[-4:] == b"--->", "scene markers")
        software = json.loads(read("software_info.conf", MAX_METADATA))
        json.loads(read("lcd_function.json", MAX_METADATA))
        info = json.loads(read("print_info.json", MAX_METADATA))
        require(info["model_layers_count"] == count, "print-info layer count")
        previews = {n: check_png(read(n)) for n in names if n.startswith("preview_images/")}
        reference_records = None
        if reference_dir:
            reference_dir = Path(reference_dir)
            reference_records = [json.loads(line) for line in (reference_dir / "layers.jsonl").read_text().splitlines()]
            require(len(reference_records) == count, "reference count")
        selected = count if sample_layers is None else min(count, sample_layers)
        require(selected > 0, "sample-layer count")
        layers = []
        previous_z = 0.0
        encoded_volume_ml = 0.0
        for index, record in enumerate(controller["paras"]):
            require(record["layer_index"] == index, "controller layer order")
            minimum, thickness, exposure = (record[k] for k in ("layer_minheight", "layer_thickness", "exposure_time"))
            require(all(math.isfinite(v) for v in (minimum, thickness, exposure)) and thickness > 0 and exposure > 0, "controller finite positive settings")
            require(math.isclose(minimum, previous_z, abs_tol=1e-4), "controller layer slab continuity")
            previous_z = minimum + thickness
            if index >= selected:
                continue
            pixels = decode_pw0(read(f"layer_images/layer_{index}.pw0Img"), width * height)
            encoded_volume_ml += sum(pixels) / 255 * machine["print_xsize"] * machine["print_ysize"] / (width * height) * thickness / 1000
            z, area, xmin, ymin, xmax, ymax, objects, largest = struct.unpack_from("<6fIf", scene, 396 + index * 64)
            require(all(math.isfinite(v) for v in (z, area, xmin, ymin, xmax, ymax, largest)), "scene finite values")
            if not commercial:
                pixel_area = machine["print_xsize"] * machine["print_ysize"] / (width * height)
                nonzero = sum(value != 0 for value in pixels)
                require(math.isclose(area, nonzero * pixel_area, rel_tol=2e-6, abs_tol=1e-5), "Karasu raster scene area")
                require(math.isclose(z, previous_z, rel_tol=2e-6, abs_tol=1e-6), "Karasu scene layer Z")
                if width * height <= 100_000:
                    actual_objects, max_component = raster_components(pixels, width, height)
                    require(objects == actual_objects and math.isclose(largest, max_component * pixel_area, rel_tol=2e-6, abs_tol=1e-5), "Karasu scene connected components")
            if reference_records is not None:
                reference = reference_records[index]
                rw, rh, rp = PNG.decode((reference_dir / reference["image"]).read_bytes())
                require((rw, rh) == (width, height) and pixels == rp, "PP1/reference mask samples")
                expected = reference["layer"]
                require(math.isclose(expected["position_z_mm"], previous_z) and math.isclose(expected["settings"]["exposure_s"], exposure), "PP1/reference effective layer settings")
            layers.append({"index": index, "z_mm": previous_z, "exposure_s": exposure, "mask_sha256": hashlib.sha256(pixels).hexdigest(), "nonzero_pixels": sum(v != 0 for v in pixels)})
        if not commercial and selected == count:
            require(math.isclose(info["volume"], encoded_volume_ml, rel_tol=2e-12, abs_tol=1e-15), "PP1 encoded coverage volume")
        return {"format": "anycubic-pp1-v3", "resolution": [width, height], "layers": count, "decoded_layers": selected, "commercial_scene_semantics": commercial, "previews": previews, "software": software["mark"], "sha256": digest, "records": layers}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path")
    parser.add_argument("--reference-dir")
    parser.add_argument("--sample-layers", type=int)
    parser.add_argument("--commercial", action="store_true")
    arguments = parser.parse_args()
    print(json.dumps(validate(arguments.path, arguments.reference_dir, arguments.sample_layers, arguments.commercial), sort_keys=True))
