"""Pack the application's PNG sizes into the Windows icon (assets/ui/icon/karasu.ico).

Each image is stored as a PNG entry (Windows Vista and later read them), so the icon keeps the
resampled pixels exactly: 16, 24, 32, 48, 64, 128 and 256 pixels square, 32-bit.
"""
import struct
import sys
from pathlib import Path

SIZES = (16, 24, 32, 48, 64, 128, 256)


def main() -> None:
    folder = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2] / "assets/ui/icon"
    images = [(folder / f"karasu-{size}.png").read_bytes() for size in SIZES]
    for size, data in zip(SIZES, images):
        width, height = struct.unpack(">II", data[16:24])
        if (width, height) != (size, size):
            raise SystemExit(f"karasu-{size}.png is {width} x {height}")
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, blobs = b"", b""
    for size, data in zip(SIZES, images):
        side = 0 if size == 256 else size  # 0 means 256 in an icon directory.
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    (folder / "karasu.ico").write_bytes(header + entries + blobs)
    print(f"wrote {folder / 'karasu.ico'}: {len(images)} images")


if __name__ == "__main__":
    main()
