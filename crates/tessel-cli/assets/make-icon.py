#!/usr/bin/env python3
"""Draws icon.png, Tessel's app icon: the logo's four tiles
(website/src/assets/logo.svg) on a white rounded square, sized like a macOS
app icon (824 of 1024 pixels). Needs Pillow: python3 make-icon.py"""

from PIL import Image, ImageDraw

SIZE, SS = 1024, 4  # drawn 4x larger, then scaled down, for smooth edges


def rounded(draw, x, y, w, h, r, fill):
    draw.rounded_rectangle([x * SS, y * SS, (x + w) * SS - 1, (y + h) * SS - 1], r * SS, fill=fill)


img = Image.new("RGBA", (SIZE * SS, SIZE * SS), (0, 0, 0, 0))
draw = ImageDraw.Draw(img)
rounded(draw, 100, 100, 824, 824, 185, "#FFFFFF")
# The logo is 32 units wide with its tiles at 3 and 17; one unit is 20 pixels here.
unit, origin = 20, (SIZE - 32 * 20) // 2
tiles = [(3, 3, "#0A84FF"), (17, 3, "#5E5CE6"), (3, 17, "#5E5CE6"), (17, 17, "#BF5AF2")]
for x, y, color in tiles:
    rounded(draw, origin + x * unit, origin + y * unit, 12 * unit, 12 * unit, 3 * unit, color)
img.resize((SIZE, SIZE), Image.LANCZOS).save("icon.png", optimize=True)
