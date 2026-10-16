#!/usr/bin/env python3
"""Generate the KVN source icon (1024x1024 PNG) with the standard library only.

Terminal-console look: graphite background, restrained cyan glyph "K",
thin steel border. `tauri icon` derives every platform icon from this file.
"""

import struct
import zlib

SIZE = 1024
BG = (10, 13, 16, 255)          # #0a0d10
BG_INNER = (16, 21, 26, 255)    # #10151a
ACCENT = (70, 200, 209, 255)    # #46c8d1
BORDER = (42, 58, 73, 255)      # #2a3a49


def dist_to_segment(px, py, x1, y1, x2, y2):
    dx, dy = x2 - x1, y2 - y1
    if dx == 0 and dy == 0:
        return ((px - x1) ** 2 + (py - y1) ** 2) ** 0.5
    t = max(0.0, min(1.0, ((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)))
    cx, cy = x1 + t * dx, y1 + t * dy
    return ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5


def inside_rounded_rect(px, py, x0, y0, x1, y1, r):
    if px < x0 or px > x1 or py < y0 or py > y1:
        return False
    for cx, cy in ((x0 + r, y0 + r), (x1 - r, y0 + r), (x0 + r, y1 - r), (x1 - r, y1 - r)):
        if (px < x0 + r or px > x1 - r) and (py < y0 + r or py > y1 - r):
            if ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5 > r:
                return False
    return True


def pixel(px, py):
    # outer border ring
    outer = inside_rounded_rect(px, py, 40, 40, SIZE - 41, SIZE - 41, 120)
    inner = inside_rounded_rect(px, py, 74, 74, SIZE - 75, SIZE - 75, 96)
    if outer and not inner:
        return BORDER
    if not outer:
        return BG
    base = BG_INNER

    # glyph K
    thickness = 62
    strokes = (
        ((330, 240, 330, 784), ACCENT),   # stem
        ((330, 512, 700, 250), ACCENT),   # upper arm
        ((330, 512, 700, 774), ACCENT),   # lower leg
    )
    for (x1, y1, x2, y2), color in strokes:
        if dist_to_segment(px, py, x1, y1, x2, y2) <= thickness:
            return color
    return base


def main():
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)  # filter: none
        for x in range(SIZE):
            raw += bytes(pixel(x, y))

    def chunk(tag, data):
        out = struct.pack(">I", len(data)) + tag + data
        out += struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        return out

    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    path = "apps/desktop/icons-src/icon.png"
    import os

    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", ihdr))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
        f.write(chunk(b"IEND", b""))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
