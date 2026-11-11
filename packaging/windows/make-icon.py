#!/usr/bin/env python3
"""Build assets/windows/lychee.ico and lychee.res from assets/icons/lychee.svg.

Needs ImageMagick (`magick`). Run it again after changing the logo and commit both outputs.
The .res is a compiled Windows resource file holding the icon; the MSVC linker accepts it
directly (crates/lychee/build.rs passes it), so no resource compiler is needed to build.

Small sizes are stored as 32-bit bitmaps and the large ones as PNG, like Visual Studio does.
"""

import pathlib
import struct
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SVG = ROOT / "assets" / "icons" / "lychee.svg"
OUT = ROOT / "assets" / "windows"
SIZES = [16, 24, 32, 48, 64, 128, 256]
PNG_FROM = 128


def magick(*args: str) -> bytes:
    return subprocess.run(["magick", *args], check=True, capture_output=True).stdout


def render(size: int) -> bytes:
    """PNG of the logo at `size` pixels."""
    return magick("-background", "none", "-density", "384", str(SVG), "-resize", f"{size}x{size}", "png32:-")


def bitmap(png: bytes, size: int) -> bytes:
    """A 32-bit DIB as stored in icons: header, bottom-up BGRA, then the AND mask.

    Windows draws 32-bit icons from their alpha channel; the mask (1 = transparent) is for
    tools and renderers that still use it."""
    pixels = subprocess.run(
        ["magick", "png:-", "-flip", "-depth", "8", "BGRA:-"], input=png, check=True, capture_output=True
    ).stdout
    assert len(pixels) == size * size * 4, (size, len(pixels))
    stride = ((size + 31) // 32) * 4
    mask = bytearray(stride * size)
    for y in range(size):
        for x in range(size):
            if pixels[(y * size + x) * 4 + 3] == 0:
                mask[y * stride + x // 8] |= 0x80 >> (x % 8)
    mask = bytes(mask)
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(pixels) + len(mask), 0, 0, 0, 0)
    return header + pixels + mask


def main() -> int:
    images = []
    for size in SIZES:
        png = render(size)
        images.append((size, png if size >= PNG_FROM else bitmap(png, size)))

    # .ico: ICONDIR, one ICONDIRENTRY per image, then the images.
    count = len(images)
    offset = 6 + 16 * count
    entries, blobs = b"", b""
    for size, data in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset + len(blobs))
        blobs += data
    ico = struct.pack("<HHH", 0, 1, count) + entries + blobs
    (OUT / "lychee.ico").write_bytes(ico)

    # .res: an empty marker entry, one RT_ICON per image and an RT_GROUP_ICON pointing at them.
    def resource(type_id: int, name_id: int, data: bytes, flags: int) -> bytes:
        header = struct.pack("<II", len(data), 32) + struct.pack("<HHHH", 0xFFFF, type_id, 0xFFFF, name_id)
        header += struct.pack("<IHHII", 0, flags, 0x0409, 0, 0)
        assert len(header) == 32
        return header + data + bytes((-len(data)) % 4)

    res = struct.pack("<II", 0, 32) + struct.pack("<HHHH", 0xFFFF, 0, 0xFFFF, 0) + struct.pack("<IHHII", 0, 0, 0, 0, 0)
    group = struct.pack("<HHH", 0, 1, count)
    for i, (size, data) in enumerate(images, start=1):
        res += resource(3, i, data, 0x1010)  # RT_ICON, MOVEABLE | DISCARDABLE
        dim = 0 if size >= 256 else size
        group += struct.pack("<BBBBHHIH", dim, dim, 0, 0, 1, 32, len(data), i)
    res += resource(14, 1, group, 0x1030)  # RT_GROUP_ICON, MOVEABLE | PURE | DISCARDABLE
    (OUT / "lychee.res").write_bytes(res)

    print(f"lychee.ico: {len(ico)} bytes, lychee.res: {len(res)} bytes, {count} sizes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
