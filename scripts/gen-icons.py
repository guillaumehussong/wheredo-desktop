#!/usr/bin/env python3
"""Generate Wheredo icons — minimal pointer + target dot (macOS menu-bar style).

Silhouette mark: a small cursor arrow with a dot at the tip ("where to click").
Tray icons are monochrome; app icons use the same mark on a soft dark tile.
Pure stdlib (zlib + struct).
"""
import math
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")

# Template white — macOS inverts for light/dark menu bars.
INK = (255, 255, 255)
# App-tile background (subtle, not the old heavy disc).
TILE = (28, 28, 38)


def png_bytes(width, height, rgba_rows):
    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        c += struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        return c

    raw = b"".join(b"\x00" + row for row in rgba_rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def aa(dist, half_width):
    """Anti-aliased 1D coverage for a stroke of given half-width."""
    if dist <= half_width - 0.75:
        return 1.0
    if dist >= half_width + 0.75:
        return 0.0
    return (half_width + 0.75 - dist) / 1.5


def dot_alpha(px, py, cx, cy, r):
    return aa(math.hypot(px - cx, py - cy), r)


def seg_alpha(px, py, x1, y1, x2, y2, half_w):
    """Distance from point to line segment → stroke coverage."""
    dx, dy = x2 - x1, y2 - y1
    len2 = dx * dx + dy * dy
    if len2 < 1e-9:
        return aa(math.hypot(px - x1, py - y1), half_w)
    t = max(0.0, min(1.0, ((px - x1) * dx + (py - y1) * dy) / len2))
    proj_x = x1 + t * dx
    proj_y = y1 + t * dy
    return aa(math.hypot(px - proj_x, py - proj_y), half_w)


def tri_alpha(px, py, v0, v1, v2):
    """Filled triangle via barycentric sign test (with AA via edge distance)."""
    def edge(a, b, p):
        return (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])

    w0 = edge(v1, v2, (px, py))
    w1 = edge(v2, v0, (px, py))
    w2 = edge(v0, v1, (px, py))
    if (w0 >= 0 and w1 >= 0 and w2 >= 0) or (w0 <= 0 and w1 <= 0 and w2 <= 0):
        # Inside — soften edges
        m = min(abs(w0), abs(w1), abs(w2))
        return min(1.0, m * 0.35 + 0.65)
    return 0.0


def mark_geometry(size):
    """Cursor tip + arrow body + target dot — fills the canvas like SF Symbols."""
    s = size
    pad = s * 0.08
    box = s - 2 * pad
    def pt(nx, ny):
        # ny=0 top, ny=1 bottom (image coords)
        return pad + nx * box, pad + ny * box

    tip_x, tip_y = pt(0.06, 0.06)
    inner_x, inner_y = pt(0.46, 0.60)
    tail_x, tail_y = pt(0.14, 0.94)
    notch_x, notch_y = pt(0.60, 0.70)
    dot_x = tip_x + box * 0.07
    dot_y = tip_y + box * 0.07
    return {
        "tip": (tip_x, tip_y),
        "inner": (inner_x, inner_y),
        "tail": (tail_x, tail_y),
        "notch": (notch_x, notch_y),
        "dot": (dot_x, dot_y),
        "dot_r": box * 0.11,
        "stroke": max(0.9, box * 0.06),
    }


def mark_alpha(px, py, geom, dot_tint=None):
    """Combined alpha for pointer + dot. dot_tint replaces white when set."""
    a = 0.0
    # Pointer body (filled wedge)
    a = max(a, tri_alpha(px, py, geom["tip"], geom["notch"], geom["inner"]))
    a = max(a, tri_alpha(px, py, geom["tip"], geom["inner"], geom["tail"]))
    a = max(a, tri_alpha(px, py, geom["inner"], geom["tail"], geom["notch"]))
    # Outline strokes for crispness at small sizes
    hw = geom["stroke"] * 0.5
    for a_pt, b_pt in [
        (geom["tip"], geom["inner"]),
        (geom["tip"], geom["tail"]),
        (geom["inner"], geom["tail"]),
        (geom["inner"], geom["notch"]),
        (geom["notch"], geom["tail"]),
    ]:
        a = max(a, seg_alpha(px, py, *a_pt, *b_pt, hw))
    # Target dot
    a = max(a, dot_alpha(px, py, *geom["dot"], geom["dot_r"]))
    return min(1.0, a)


def draw_mark(size, ink=INK, tile=None, dot_tint=None):
    geom = mark_geometry(size)
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            px, py = x + 0.5, y + 0.5
            r = g = b = alpha = 0.0
            if tile:
                # Rounded tile for app icons
                cx = cy = size / 2.0
                corner_r = size * 0.22
                dx = max(abs(px - cx) - (size / 2 - corner_r), 0)
                dy = max(abs(py - cy) - (size / 2 - corner_r), 0)
                if math.hypot(dx, dy) <= corner_r:
                    r, g, b, alpha = tile[0], tile[1], tile[2], 1.0
            ma = mark_alpha(px, py, geom, dot_tint)
            if ma > 0:
                if dot_tint and dot_alpha(px, py, *geom["dot"], geom["dot_r"]) > 0.5:
                    ir, ig, ib = dot_tint
                else:
                    ir, ig, ib = ink
                r = ir * ma + r * (1 - ma)
                g = ig * ma + g * (1 - ma)
                b = ib * ma + b * (1 - ma)
                alpha = max(alpha, ma)
            row += bytes((int(r), int(g), int(b), int(alpha * 255)))
        rows.append(bytes(row))
    return png_bytes(size, size, rows)


def draw_tray(size, dot=(255, 255, 255)):
    """Monochrome tray icon; optional tinted dot for state."""
    geom = mark_geometry(size)
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            px, py = x + 0.5, y + 0.5
            ma = mark_alpha(px, py, geom)
            da = dot_alpha(px, py, *geom["dot"], geom["dot_r"])
            # Pointer always white; dot may be tinted
            r = g = b = 0
            a = 0.0
            pointer_a = ma * (1 - da * 0.85)  # dot area overrides
            if pointer_a > 0:
                r = g = b = int(255 * pointer_a)
                a = pointer_a
            if da > 0:
                dr, dg, db = dot
                blend = da
                r = int(dr * blend + r * (1 - blend))
                g = int(dg * blend + g * (1 - blend))
                b = int(db * blend + b * (1 - blend))
                a = max(a, da)
            row += bytes((r, g, b, int(a * 255)))
        rows.append(bytes(row))
    return png_bytes(size, size, rows)


def ico_from_png(png_data, size):
    header = struct.pack("<HHH", 0, 1, 1)
    s = 0 if size >= 256 else size
    entry = struct.pack("<BBBBHHII", s, s, 0, 0, 1, 32, len(png_data), 22)
    return header + entry + png_data


def main():
    os.makedirs(OUT, exist_ok=True)

    for name, size in [("32x32.png", 32), ("128x128.png", 128),
                       ("128x128@2x.png", 256), ("icon.png", 512)]:
        with open(os.path.join(OUT, name), "wb") as f:
            f.write(draw_mark(size, tile=TILE))

    with open(os.path.join(OUT, "icon.ico"), "wb") as f:
        f.write(ico_from_png(draw_mark(256, tile=TILE), 256))

    states = {
        "tray-ready.png": (255, 255, 255),
        "tray-listening.png": (255, 255, 255),
        "tray-busy.png": (255, 200, 80),
        "tray-error.png": (160, 160, 168),
    }
    for name, dot in states.items():
        with open(os.path.join(OUT, name), "wb") as f:
            f.write(draw_tray(32, dot=dot))

    print(f"Icons written to {os.path.abspath(OUT)}")


if __name__ == "__main__":
    main()
