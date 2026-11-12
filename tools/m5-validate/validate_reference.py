#!/usr/bin/env python3
"""Independently read reference job metadata/PNG masks; no production Rust helpers are imported."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import zlib

PNG_MODULE = Path(__file__).resolve().parents[1] / "m4-profile" / "validate_png.py"
SPEC = importlib.util.spec_from_file_location("independent_png", PNG_MODULE)
PNG = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PNG)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate(directory):
    directory = Path(directory)
    manifest = json.loads((directory / "manifest.json").read_text())
    require(manifest["schema_version"] == 1, "unsupported reference schema")
    require(manifest["application"]["name"] == "karasu-slicer", "wrong application identity")
    require(manifest["layers"] == "layers.jsonl", "unexpected layer index path")
    printer = manifest["printer"]
    require(printer["schema_version"] == manifest["resin"]["schema_version"] == 1, "profile schema")
    width, height = printer["resolution_px"]
    require(0 < width <= 65536 and 0 < height <= 65536, "invalid LCD dimensions")
    require(len(manifest["overrides"]) <= manifest["layer_count"], "override count")
    count = 0
    volume = 0.0
    payload_bytes = (directory / "layers.jsonl").stat().st_size
    with (directory / "layers.jsonl").open() as lines:
        for line in lines:
            record = json.loads(line)
            layer = record["layer"]
            require(layer["index"] == count, "layer index/order")
            expected_name = f"layer-{count:08}.png"
            require(record["image"] == expected_name, "unexpected image path")
            data = (directory / expected_name).read_bytes()
            actual_width, actual_height, pixels = PNG.decode(data)
            require((actual_width, actual_height) == (width, height), "mask resolution")
            require(zlib.crc32(pixels) == record["mask_crc32"], "mask CRC mismatch")
            total = sum(pixels)
            require(total == record["statistics"]["intensity_sum"], "mask intensity sum")
            require(sum(value != 0 for value in pixels) == record["statistics"]["nonzero_pixels"], "nonzero count")
            thickness, position, sample = (layer[key] for key in ("thickness_mm", "position_z_mm", "sample_z_mm"))
            require(all(math.isfinite(v) for v in (thickness, position, sample)), "nonfinite layer")
            require(thickness > 0 and position > 0, "nonpositive layer thickness/position")
            tolerance = 8 * math.ulp(max(abs(position), thickness))
            require(position - thickness - tolerance <= sample <= position + tolerance, "sample slab")
            settings = layer["settings"]
            require(0 < settings["pwm"] <= 255 and settings["exposure_s"] > 0, "exposure/PWM")
            lift = sum(stage["distance_mm"] for stage in settings["lift"])
            retract = sum(stage["distance_mm"] for stage in settings["retract"])
            require(abs(lift - retract) <= 1e-9 * max(lift, retract), "motion travel mismatch")
            require(position + lift <= printer["build_size_mm"][2], "Z travel exceeded")
            raster = record["raster"]
            require(raster["samples_per_axis"] in (1, 2, 4, 8, 16), "AA samples")
            require(2 <= raster["grayscale_levels"] <= 256, "gray levels")
            # Optional since TD-060: partial coverage maps onto minimum..255, then rounds to the
            # nearest level, so no nonzero pixel lies more than half a level step below it.
            # The field is omitted when zero.
            minimum = raster.get("minimum_gray", 0)
            require(isinstance(minimum, int) and ("minimum_gray" not in raster or 1 <= minimum <= 255), "minimum gray")
            if minimum:
                step = 255 / (raster["grayscale_levels"] - 1)
                require(all(value >= minimum - step / 2 for value in pixels if value), "pixel below minimum gray")
            require(raster["outside"] in ("clip", "reject"), "outside policy")
            require(raster["compensation"]["kind"] in ("none", "dilate", "erode"), "compensation")
            area = printer["build_size_mm"][0] * printer["build_size_mm"][1] / (width * height)
            volume += total / 255 * area * thickness
            payload_bytes += len(data)
            count += 1
    require(count == manifest["layer_count"] == manifest["summary"]["layers"], "layer count")
    require(payload_bytes == manifest["summary"]["payload_bytes_excluding_manifest"], "payload byte count")
    require(math.isclose(volume, manifest["summary"]["coverage_volume_mm3"], rel_tol=1e-12, abs_tol=1e-12), "coverage volume")
    expected = {"manifest.json", "layers.jsonl"} | {f"layer-{i:08}.png" for i in range(count)}
    require({path.name for path in directory.iterdir()} == expected, "unexpected reference members")
    return {"layers": count, "resolution": [width, height], "source_coverage_volume_mm3": volume,
            "bytes": payload_bytes + (directory / "manifest.json").stat().st_size}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory")
    args = parser.parse_args()
    print(json.dumps(validate(args.directory), sort_keys=True))
