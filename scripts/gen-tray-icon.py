#!/usr/bin/env python3
"""Generate the menu-bar tray icon (template PNG) with the standard library.

A solid shield with a keyhole cut out, black-on-transparent. macOS treats
template images specially: it renders them in the menu-bar colour and
inverts them for dark/light themes, so only the alpha shape matters. The
same shape reads well on Windows/Linux trays.

Rendered at 36x36 with 4x supersampling (crisp at the 18pt menu-bar height
on Retina). `tray-icon` normalises the height to 18pt itself.
"""

import math
import os
import struct
import zlib

OUT = "apps/desktop/src-tauri/icons/tray-template.png"
SIZE = 36
SS = 4  # supersampling factor per axis


def shield_polygon():
    """Closed shield outline in normalised coordinates (y grows down)."""
    pts = []

    def quad(p0, p1, p2, steps):
        for i in range(steps + 1):
            t = i / steps
            x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t * t * p2[0]
            y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t * t * p2[1]
            pts.append((x, y))

    def arc(cx, cy, r, a0, a1, steps):
        for i in range(steps + 1):
            a = math.radians(a0 + (a1 - a0) * i / steps)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))

    # Top-right corner → right side → taper to the bottom tip → mirrored
    # left side → top-left corner; the polygon closes along the top edge.
    arc(0.76, 0.18, 0.10, 270, 360, 8)
    pts.append((0.86, 0.50))
    quad((0.86, 0.50), (0.84, 0.76), (0.50, 0.94), 12)
    quad((0.50, 0.94), (0.16, 0.76), (0.14, 0.50), 12)
    pts.append((0.14, 0.18))
    arc(0.24, 0.18, 0.10, 180, 270, 8)
    return pts


def inside_polygon(px, py, poly):
    inside = False
    count = len(poly)
    for i in range(count):
        x1, y1 = poly[i]
        x2, y2 = poly[(i + 1) % count]
        if (y1 > py) != (y2 > py):
            x_cross = x1 + (py - y1) * (x2 - x1) / (y2 - y1)
            if px < x_cross:
                inside = not inside
    return inside


def inside_keyhole(px, py):
    """Circle plus a tapering slot = classic keyhole."""
    dx, dy = px - 0.5, py - 0.355
    if dx * dx + dy * dy <= 0.088 * 0.088:
        return True
    if 0.36 <= py <= 0.635:
        t = (py - 0.36) / (0.635 - 0.36)
        half = 0.052 + (0.026 - 0.052) * t
        return abs(px - 0.5) <= half
    return False


def main():
    poly = shield_polygon()
    full = SIZE * SS

    raw = bytearray()
    covered = 0
    for oy in range(SIZE):
        raw.append(0)  # PNG filter: none
        for ox in range(SIZE):
            hits = 0
            for sy in range(SS):
                py = (oy * SS + sy + 0.5) / full
                for sx in range(SS):
                    px = (ox * SS + sx + 0.5) / full
                    if inside_polygon(px, py, poly) and not inside_keyhole(px, py):
                        hits += 1
            alpha = round(255 * hits / (SS * SS))
            covered += alpha > 0
            raw += bytes((0, 0, 0, alpha))

    def chunk(tag, data):
        out = struct.pack(">I", len(data)) + tag + data
        return out + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "wb") as fh:
        fh.write(png)
    print(f"wrote {OUT} ({SIZE}x{SIZE}, {len(png)} bytes, {covered} covered pixels)")


if __name__ == "__main__":
    main()
