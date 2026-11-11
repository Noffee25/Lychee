#!/usr/bin/env python3
"""Build assets/macos/lychee.icns from assets/icons/lychee.svg.

macOS app icons are rounded squares, and recent macOS versions put icons that don't fill that
shape on a grey tile. So here the logo sits on a dark tile that follows Apple's icon grid: an
824 px superellipse centred in a 1024 px canvas, with the margin left for the system's shadow.

Needs ImageMagick (`magick`) with SVG support. Run it again after changing the logo and commit
the output. `--preview FILE.png` also writes the 1024 px image, to look at.
"""

import math
import re
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SVG = ROOT / "assets" / "icons" / "lychee.svg"
OUT = ROOT / "assets" / "macos" / "lychee.icns"

# ICNS entries that hold PNG data: (type, pixels). The @2x types are the Retina versions.
ENTRIES = [
    (b"icp4", 16),
    (b"ic11", 32),  # 16@2x
    (b"icp5", 32),
    (b"ic12", 64),  # 32@2x
    (b"ic07", 128),
    (b"ic13", 256),  # 128@2x
    (b"ic08", 256),
    (b"ic14", 512),  # 256@2x
    (b"ic09", 512),
    (b"ic10", 1024),  # 512@2x
]


def magick(*args: str, data: bytes | None = None) -> bytes:
    return subprocess.run(["magick", *args], input=data, check=True, capture_output=True).stdout


def superellipse(cx: float, cy: float, radius: float, n: float = 5.0, steps: int = 720) -> str:
    """An SVG path close to Apple's icon shape (a rounded square with continuous corners)."""
    points = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        x = cx + radius * math.copysign(abs(c) ** (2 / n), c)
        y = cy + radius * math.copysign(abs(s) ** (2 / n), s)
        points.append(f"{x:.2f},{y:.2f}")
    return "M" + " L".join(points) + " Z"


def icon_svg() -> str:
    logo = SVG.read_text()
    inner = re.sub(r"^.*?<svg[^>]*>", "", logo, count=1, flags=re.S)
    inner = inner[: inner.rindex("</svg>")]
    # The logo is drawn on a 32-unit grid: the fruit is centred at (15, 17.5) with a radius of 12
    # and the leaf reaches up to the right. At 21.5 px per unit the whole logo is about 520 px
    # wide, centred in the tile.
    scale = 21.5
    x = 512 - 15.1 * scale
    y = 512 - 16.15 * scale
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#2F2128"/>
      <stop offset="1" stop-color="#140D11"/>
    </linearGradient>
  </defs>
  <path d="{superellipse(512, 512, 412)}" fill="url(#tile)"/>
  <svg x="{x:.1f}" y="{y:.1f}" width="{32 * scale:.1f}" height="{32 * scale:.1f}" viewBox="0 0 32 32">{inner}</svg>
</svg>
"""


def main() -> None:
    no_dates = ("-strip", "-define", "png:exclude-chunks=date,time")
    master = magick("-background", "none", "svg:-", *no_dates, "png32:-", data=icon_svg().encode())
    pngs = {1024: master}
    for _, size in ENTRIES:
        if size not in pngs:
            pngs[size] = magick("png:-", "-filter", "Lanczos", "-resize", f"{size}x{size}", *no_dates, "png32:-", data=master)
    body = b"".join(kind + struct.pack(">I", 8 + len(pngs[size])) + pngs[size] for kind, size in ENTRIES)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size // 1024} KB)")
    if len(sys.argv) == 3 and sys.argv[1] == "--preview":
        Path(sys.argv[2]).write_bytes(master)


if __name__ == "__main__":
    main()
