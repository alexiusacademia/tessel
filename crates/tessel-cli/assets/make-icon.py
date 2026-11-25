#!/usr/bin/env python3
"""Draws Tessel's logo everywhere it's used, from one description of it:

- website/src/assets/logo.svg: the mark alone (the site's header and home page)
- website/public/favicon.svg: the mark on a dark rounded square
- crates/tessel-cli/assets/icon.png: the app icon, sized like a macOS app
  icon (824 of 1024 pixels), for apps that don't have their own
- editors/vscode/icon.png: the VS Code extension's icon (128 pixels)

The mark is a T inside two interlocking outlines, drawn as lines on a grid
of 6 × 6 points, in cyan on a dark gray. Needs Pillow: python3 make-icon.py
(from anywhere; the paths are found from this file's place)."""

from pathlib import Path

from PIL import Image, ImageDraw

CYAN, DARK = "#40C0DB", "#212429"
# A line's width, in grid steps.
STROKE = 0.3275
# The lines, as points on the grid (column, row). The T is closed; the
# outline behind it, top right, and the one in front, bottom left, start
# and end on the T.
T = [(1, 1), (4, 1), (4, 2), (3, 2), (3, 4), (2, 4), (2, 2), (1, 2), (1, 1)]
BACK = [(1, 1), (1, 0), (4, 0), (5, 1), (5, 2), (4, 2)]
FRONT = [(1, 1), (0, 1), (0, 3), (2, 5), (4, 5), (4, 2)]
LINES = [(T, True), (BACK, False), (FRONT, False)]
# How wide the mark is with its lines' width, in grid steps.
SPAN = 5 + STROKE

REPO = Path(__file__).resolve().parents[3]


def svg_paths(size, origin):
    """The lines as SVG path data, with grid steps of `size` from `origin`."""
    out = []
    for points, closed in LINES:
        pts = points[:-1] if closed else points
        d = "M" + " L".join(f"{fmt(origin + x * size)} {fmt(origin + y * size)}" for x, y in pts)
        out.append(d + (" Z" if closed else ""))
    return out


def fmt(v):
    return f"{v:.3f}".rstrip("0").rstrip(".")


def svg(width, mark, tile=None):
    """An SVG `width` units square with the mark `mark` units wide in its
    middle, on a dark rounded square with corner radius `tile`, or alone."""
    size = mark / SPAN
    origin = (width - mark) / 2 + STROKE * size / 2
    lines = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {width}" fill="none">']
    if tile is not None:
        lines.append(f'  <rect width="{width}" height="{width}" rx="{tile}" fill="{DARK}"/>')
    lines.append(f'  <g stroke="{CYAN}" stroke-width="{fmt(STROKE * size)}" stroke-linejoin="miter" stroke-linecap="square">')
    for d in svg_paths(size, origin):
        lines.append(f'    <path d="{d}"/>')
    lines.append("  </g>")
    lines.append("</svg>")
    return "\n".join(lines) + "\n"


def outline(points, closed, half):
    """The polygons that cover a line `half` wide on each side: one per
    segment and a mitered wedge at each corner. An open line's two ends
    are square: they go on half a width past its first and last points."""
    polys = []
    n = len(points) - 1
    for i in range(n):
        (x0, y0), (x1, y1) = points[i], points[i + 1]
        length = ((x1 - x0) ** 2 + (y1 - y0) ** 2) ** 0.5
        dx, dy = (x1 - x0) / length * half, (y1 - y0) / length * half
        start = 1 if i == 0 and not closed else 0
        end = 1 if i == n - 1 and not closed else 0
        a = (x0 - dx * start, y0 - dy * start)
        b = (x1 + dx * end, y1 + dy * end)
        polys.append([(a[0] - dy, a[1] + dx), (b[0] - dy, b[1] + dx), (b[0] + dy, b[1] - dx), (a[0] + dy, a[1] - dx)])
    corners = range(n) if closed else range(1, n)
    for i in corners:
        p = points[i]
        prev, nxt = points[i - 1] if i > 0 else points[n - 1], points[i + 1]
        for side in (1, -1):
            n1 = normal(prev, p, half * side)
            n2 = normal(p, nxt, half * side)
            m = miter(prev, p, nxt, n1, n2)
            polys.append([p, (p[0] + n1[0], p[1] + n1[1]), m, (p[0] + n2[0], p[1] + n2[1])])
    return polys


def normal(a, b, h):
    dx, dy = b[0] - a[0], b[1] - a[1]
    length = (dx * dx + dy * dy) ** 0.5
    return (-dy / length * h, dx / length * h)


def miter(a, p, b, n1, n2):
    """Where the offset lines before and after corner `p` meet."""
    d1 = (p[0] - a[0], p[1] - a[1])
    d2 = (b[0] - p[0], b[1] - p[1])
    p1 = (p[0] + n1[0], p[1] + n1[1])
    p2 = (p[0] + n2[0], p[1] + n2[1])
    det = d1[0] * -d2[1] - d1[1] * -d2[0]
    if abs(det) < 1e-9:
        return p1
    t = ((p2[0] - p1[0]) * -d2[1] - (p2[1] - p1[1]) * -d2[0]) / det
    return (p1[0] + d1[0] * t, p1[1] + d1[1] * t)


def png(path, pixels, tile_inset, tile_radius, mark):
    """A PNG `pixels` square: a dark rounded square `tile_inset` from the
    edges, with the mark `mark` pixels wide in its middle."""
    ss = 4  # drawn 4x larger, then scaled down, for smooth edges
    big = pixels * ss
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle([tile_inset * ss, tile_inset * ss, big - tile_inset * ss - 1, big - tile_inset * ss - 1], tile_radius * ss, fill=DARK)
    size = mark * ss / SPAN
    origin = (big - mark * ss) / 2 + STROKE * size / 2
    for points, closed in LINES:
        scaled = [(origin + x * size, origin + y * size) for x, y in points]
        for poly in outline(scaled, closed, STROKE * size / 2):
            draw.polygon(poly, fill=CYAN)
    img.resize((pixels, pixels), Image.LANCZOS).save(path, optimize=True)


(REPO / "website/src/assets/logo.svg").write_text(svg(32, 30))
(REPO / "website/public/favicon.svg").write_text(svg(32, 21, tile=7))
png(REPO / "crates/tessel-cli/assets/icon.png", 1024, 100, 185, 480)
png(REPO / "editors/vscode/icon.png", 128, 0, 28, 84)
