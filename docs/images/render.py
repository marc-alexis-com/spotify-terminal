"""Renders the README images of spotify-terminal (PNG, drawn at 2x).

    python3 render.py OUT_DIR FONTS_DIR CAPTURES_DIR

Needs Pillow. FONTS_DIR holds Figtree (https://github.com/erikdkennedy/figtree,
SIL OFL) as figtree.ttf, for the banner and the diagram; Iosevka
(https://github.com/be5invis/Iosevka) as iosevka-regular.ttf, iosevka-bold.ttf and
iosevka-italic.ttf, for the terminal; and a Nerd Font Mono as nerd.ttf, for the
player's icons.

CAPTURES_DIR holds real terminal output: each NAME.ansi is one screen of `spt`
captured with `tmux capture-pane -e -p`, 150 columns wide. Album covers reach the
terminal as kitty Unicode placeholders, which a capture keeps as text; NAME.covers
lists, one per line, `RRGGBB IMAGE` for the placeholders of that color (the color
is the image's ID: see src/cover.rs), and the image is drawn in their place.

Style: the player's own. Dark surfaces, Spotify's green as the one accent, a
gradient header as a playlist page has, rounded cards, a bold geometric sans.
No Spotify logo: it is a trademark.
"""
import math
import re
import sys
import unicodedata
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

OUT, FONTS, CAPTURES = (Path(arg) for arg in sys.argv[1:4])
K = 2  # supersampling: every logical pixel is K device pixels

BASE, CARD, RAISED = "#121212", "#181818", "#282828"
TEXT, SUBTEXT, MUTED = "#ffffff", "#b3b3b3", "#6a6a6a"
GREEN, GREEN_DEEP = "#1ed760", "#1db954"


def font(name, size):
    return ImageFont.truetype(str(FONTS / name), size * K)


def mono(size, bold=False):
    return font("iosevka-bold.ttf" if bold else "iosevka-regular.ttf", size)


def sans(size, weight=400):
    face = font("figtree.ttf", size)
    try:
        face.set_variation_by_axes([weight])
    except OSError:
        pass
    return face


def rgb(color):
    return tuple(int(color[i:i + 2], 16) for i in (1, 3, 5))


def mix(a, b, t):
    return tuple(round(x + (y - x) * t) for x, y in zip(rgb(a), rgb(b)))


def gradient(img, top, bottom, height):
    """A vertical fade from `top` to `bottom` over the first `height` logical pixels."""
    d = ImageDraw.Draw(img)
    for y in range(height * K):
        d.line([(0, y), (img.width, y)], fill=mix(top, bottom, y / (height * K)))


def shadow(img, box, radius, blur=18, offset=8, strength=150):
    """A soft drop shadow under a rounded box, as Spotify's cards have."""
    x0, y0, x1, y1 = (v * K for v in box)
    layer = Image.new("L", img.size, 0)
    ImageDraw.Draw(layer).rounded_rectangle([x0, y0 + offset * K, x1, y1 + offset * K], radius=radius * K, fill=strength)
    layer = layer.filter(ImageFilter.GaussianBlur(blur * K))
    img.paste(Image.new("RGB", img.size, "#000000"), (0, 0), layer)


def play_button(d, cx, cy, r):
    d.ellipse([(cx - r) * K, (cy - r) * K, (cx + r) * K, (cy + r) * K], fill=GREEN)
    s = r * 0.42
    d.polygon([((cx - s * 0.75) * K, (cy - s) * K), ((cx - s * 0.75) * K, (cy + s) * K), ((cx + s * 1.05) * K, cy * K)], fill="#000000")


# --- The cover of the banner: a terminal prompt over an equalizer ----------------------------
def cover_art(size):
    img = Image.new("RGB", (size * K, size * K), "#0d0d0d")
    gradient(img, "#1f6f43", "#0b0b0b", size)
    d = ImageDraw.Draw(img)
    # five bars, as assets/logo.txt draws the player's logo
    heights, w, gap = [0.30, 0.48, 0.66, 0.48, 0.30], size * 0.09, size * 0.045
    left = (size - (5 * w + 4 * gap)) / 2
    floor = size * 0.80
    for i, h in enumerate(heights):
        x = left + i * (w + gap)
        d.rounded_rectangle([x * K, (floor - h * size) * K, (x + w) * K, floor * K], radius=w * 0.35 * K, fill=GREEN)
    d.text((size * 0.09 * K, size * 0.07 * K), ">_", font=mono(int(size * 0.17), bold=True), fill=TEXT)
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, img.width - 1, img.height - 1], radius=10 * K, fill=255)
    return img, mask


def hero():
    W, H = 960, 420
    img = Image.new("RGB", (W * K, H * K), BASE)
    gradient(img, "#1a6b40", BASE, 330)
    d = ImageDraw.Draw(img)

    art = 232
    x0, y0 = 48, 52
    shadow(img, (x0, y0, x0 + art, y0 + art), 10, blur=22, offset=12, strength=170)
    cover, mask = cover_art(art)
    img.paste(cover, (x0 * K, y0 * K), mask)
    d = ImageDraw.Draw(img)

    x = x0 + art + 36
    d.text((x * K, 118 * K), "Terminal client", font=sans(15, 700), fill=TEXT)
    title = "spotify-terminal"
    size = 78
    while size > 30 and d.textlength(title, font=sans(size, 800)) / K > W - 40 - x:
        size -= 1
    d.text(((x - 3) * K, 134 * K), title, font=sans(size, 800), fill=TEXT)
    d.text((x * K, (148 + size) * K), "Your library, search and artists, played from a terminal.", font=sans(17, 500), fill=SUBTEXT)
    meta = ["Rust + HTML", "librespot", "320 kbit/s", "covers in kitty", "Premium"]
    mx = x
    for i, item in enumerate(meta):
        face = sans(14, 700 if i == 0 else 500)
        d.text((mx * K, (184 + size) * K), item, font=face, fill=TEXT if i == 0 else SUBTEXT)
        mx += d.textlength(item, font=face) / K
        if i < len(meta) - 1:
            d.text(((mx + 6) * K, (184 + size) * K), "•", font=face, fill=SUBTEXT)
            mx += 22

    # the action row of a playlist page
    play_button(d, x0 + 28, 330, 28)
    for i, glyph in enumerate(["\U000f049d", "\U000f0456", "\U000f01d8"]):  # shuffle, repeat, more
        d.text(((x0 + 82 + i * 44) * K, 316 * K), glyph, font=font("nerd.ttf", 26), fill=SUBTEXT)

    # a player bar along the bottom
    by = 386
    d.text((x0 * K, (by - 9) * K), "1:42", font=sans(12, 500), fill=SUBTEXT)
    left, right, at = x0 + 44, W - 92, 0.38
    d.rounded_rectangle([left * K, (by - 2) * K, right * K, (by + 2) * K], radius=2 * K, fill="#4d4d4d")
    mid = left + (right - left) * at
    d.rounded_rectangle([left * K, (by - 2) * K, mid * K, (by + 2) * K], radius=2 * K, fill=GREEN)
    d.ellipse([(mid - 6) * K, (by - 6) * K, (mid + 6) * K, (by + 6) * K], fill=TEXT)
    d.text(((right + 14) * K, (by - 9) * K), "4:28", font=sans(12, 500), fill=SUBTEXT)
    img.save(OUT / "hero.png", optimize=True)


# --- How it works, as three cards of a shelf ---------------------------------------------------
def architecture():
    W, H = 960, 450
    img = Image.new("RGB", (W * K, H * K), BASE)
    d = ImageDraw.Draw(img)
    d.text((40 * K, 30 * K), "How it works", font=sans(26, 800), fill=TEXT)
    d.text((40 * K, 66 * K), "One process between your terminal and Spotify.", font=sans(15, 500), fill=SUBTEXT)

    cards = [
        ("Your terminal", "you see and press", [("ui/*.html + CSS", "compiled at build time"), ("keys and mouse", "every key remappable"), ("kitty images", "album covers, in tmux too")]),
        ("spt", "one process", [("hypercmd views", "bound to the Rust state"), ("librespot speaker", "plays the audio itself"), ("config.toml", "theme, keys, playback")]),
        ("Spotify", "your Premium account", [("accounts", "OAuth with PKCE, once"), ("catalog and library", "through the session"), ("audio streams", "320 kbit/s, cached")]),
    ]
    cw, ch, top, gap = 272, 262, 104, 32
    for i, (title, sub, rows) in enumerate(cards):
        x = 40 + i * (cw + gap)
        fill = RAISED if i == 1 else CARD
        d.rounded_rectangle([x * K, top * K, (x + cw) * K, (top + ch) * K], radius=8 * K, fill=fill)
        d.text(((x + 20) * K, (top + 18) * K), title, font=sans(19, 800), fill=TEXT)
        d.text(((x + 20) * K, (top + 44) * K), sub, font=sans(13, 500), fill=SUBTEXT)
        for j, (name, note) in enumerate(rows):
            y = top + 86 + j * 56
            d.ellipse([(x + 20) * K, (y + 6) * K, (x + 28) * K, (y + 14) * K], fill=GREEN)
            d.text(((x + 38) * K, y * K), name, font=mono(14, bold=True), fill=TEXT)
            d.text(((x + 38) * K, (y + 22) * K), note, font=sans(13, 500), fill=SUBTEXT)
        if i < 2:
            ax = x + cw + 4
            d.line([(ax * K, (top + ch / 2) * K), ((ax + gap - 10) * K, (top + ch / 2) * K)], fill=GREEN, width=2 * K)
            mid_y = top + ch / 2
            for tip, back in ((ax + gap - 8, -7), (ax, 7)):  # both ways
                d.polygon([(tip * K, mid_y * K), ((tip + back) * K, (mid_y - 5) * K), ((tip + back) * K, (mid_y + 5) * K)], fill=GREEN)

    y = top + ch + 26
    d.rounded_rectangle([40 * K, y * K, (W - 40) * K, (y + 34) * K], radius=17 * K, fill=CARD)
    play_button(d, 58, y + 17, 11)
    d.text((80 * K, (y + 8) * K), "Media keys, playerctl and status bars drive it over MPRIS; your phone drives it over Spotify Connect.", font=sans(14, 500), fill=SUBTEXT)
    img.save(OUT / "architecture.png", optimize=True)


# --- Terminal screenshots ---------------------------------------------------------------------
SGR = re.compile(r"\x1b\[([0-9;]*)m")
PLACEHOLDER = "\U0010eeee"
BOX = set("─━│┃╭╮╰╯┌┐└┘")


def parse_ansi(text, default_fg, default_bg):
    """Returns lines of cells: (char, fg, bg, bold, italic, underline, width)."""
    lines = []
    fg, bg, bold, italic, under = default_fg, default_bg, False, False, False
    for raw in text.split("\n"):
        cells, pos = [], 0
        for m in list(SGR.finditer(raw)) + [None]:
            chunk = raw[pos:m.start()] if m else raw[pos:]
            for ch in chunk:
                if unicodedata.combining(ch) and cells:
                    continue  # a placeholder's row and column marks
                wide = unicodedata.east_asian_width(ch) in "WF"
                cells.append((ch, fg, bg, bold, italic, under, 2 if wide else 1))
            if not m:
                break
            pos = m.end()
            codes = [int(c) for c in m.group(1).split(";") if c] or [0]
            i = 0
            while i < len(codes):
                c = codes[i]
                if c == 0:
                    fg, bg, bold, italic, under = default_fg, default_bg, False, False, False
                elif c == 1:
                    bold = True
                elif c == 3:
                    italic = True
                elif c == 4:
                    under = True
                elif c in (22, 23, 24):
                    bold, italic, under = (False if c == 22 else bold), (False if c == 23 else italic), (False if c == 24 else under)
                elif c in (38, 48) and i + 4 < len(codes) and codes[i + 1] == 2:
                    color = "#%02x%02x%02x" % tuple(codes[i + 2:i + 5])
                    fg, bg = (color, bg) if c == 38 else (fg, color)
                    i += 4
                elif c == 39:
                    fg = default_fg
                elif c == 49:
                    bg = default_bg
                i += 1
        lines.append(cells)
    while lines and not any(cell[0].strip() for cell in lines[-1]):
        lines.pop()
    return lines


def box(d, ch, x, y, w, h, color):
    """Draws a box-drawing cell as lines that meet their neighbours exactly."""
    heavy = ch in "━┃"
    lw = max(1, round((2.2 if heavy else 1.1) * K))
    cx, cy = x + w / 2, y + h / 2
    left = ch in "─━┐┘╮╯"
    right = ch in "─━┌└╭╰"
    up = ch in "│┃└┘╰╯"
    down = ch in "│┃┌┐╭╮"
    if ch in "╭╮╰╯":
        r = min(w, h) / 2
        ex, ey = (x + w if right else x), (y + h if down else y)
        sx, sy = (1 if right else -1), (1 if down else -1)
        # the straight parts, then a quarter circle between them
        d.line([(cx + sx * r, cy), (ex, cy)], fill=color, width=lw)
        d.line([(cx, cy + sy * r), (cx, ey)], fill=color, width=lw)
        ox, oy = cx + sx * r, cy + sy * r
        start = {(1, 1): 180, (-1, 1): 270, (1, -1): 90, (-1, -1): 0}[(sx, sy)]
        d.arc([ox - r, oy - r, ox + r, oy + r], start, start + 90, fill=color, width=lw)
        return
    if left or right:
        d.line([(x if left else cx, cy), (x + w if right else cx, cy)], fill=color, width=lw)
    if up or down:
        d.line([(cx, y if up else cy), (cx, y + h if down else cy)], fill=color, width=lw)


def cover_image(path, w, h):
    """The image cropped to the shape of its placeholders, as kitty fills them."""
    img = Image.open(path).convert("RGB")
    side = min(img.size)
    img = img.crop(((img.width - side) // 2, (img.height - side) // 2, (img.width + side) // 2, (img.height + side) // 2))
    ratio = w / h
    if ratio < 1:
        cut = side * (1 - ratio) / 2
        img = img.crop((int(cut), 0, int(side - cut), side))
    else:
        cut = side * (1 - 1 / ratio) / 2
        img = img.crop((0, int(cut), side, int(side - cut)))
    return img.resize((int(w), int(h)), Image.LANCZOS)


def terminal(name, default_fg, default_bg):
    text = (CAPTURES / f"{name}.ansi").read_text(encoding="utf-8")
    covers = {}
    listing = CAPTURES / f"{name}.covers"
    if listing.exists():
        for line in listing.read_text().split("\n"):
            if line.strip():
                color, path = line.split(maxsplit=1)
                covers["#" + color.lower()] = CAPTURES / path
    lines = parse_ansi(text, default_fg, default_bg)
    size = 15
    faces = {
        (False, False): mono(size), (True, False): mono(size, bold=True),
        (False, True): font("iosevka-italic.ttf", size), (True, True): font("iosevka-italic.ttf", size),
    }
    nerd = font("nerd.ttf", size)
    cw = faces[(False, False)].getlength("M") / K  # Iosevka: exactly 0.5 em
    lh = round(size * 1.25)
    ncols = max(sum(cell[6] for cell in line) for line in lines)
    pad = 18
    W, H = int(pad * 2 + ncols * cw), int(pad * 2 + len(lines) * lh)
    img = Image.new("RGB", (W * K, H * K), default_bg)
    d = ImageDraw.Draw(img)
    spots = {}
    for row, line in enumerate(lines):
        col = 0
        for ch, fg, bg, bold, italic, under, width in line:
            x, y = (pad + col * cw) * K, (pad + row * lh) * K
            w, h = cw * width * K, lh * K
            if bg != default_bg:
                d.rectangle([x, y, x + w, y + h], fill=bg)
            if ch == PLACEHOLDER:
                spots.setdefault(fg, []).append((x, y))
            elif ch in BOX:
                box(d, ch, x, y, w, h, fg)
            elif ch != " ":
                face = nerd if 0xE000 <= ord(ch) <= 0xF8FF or ord(ch) >= 0xF0000 else faces[(bold, italic)]
                d.text((x, y + (h - size * K * 1.18) / 2), ch, font=face, fill=fg)
            if under and ch != " ":
                d.line([(x, y + h - 3 * K), (x + w, y + h - 3 * K)], fill=fg, width=K)
            col += width
    for color, cells in spots.items():
        if color not in covers:
            continue
        x0, y0 = min(c[0] for c in cells), min(c[1] for c in cells)
        x1, y1 = max(c[0] for c in cells) + cw * K, max(c[1] for c in cells) + lh * K
        img.paste(cover_image(covers[color], x1 - x0, y1 - y0), (int(x0), int(y0)))
    mask = Image.new("L", img.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, img.width - 1, img.height - 1], radius=12 * K, fill=255)
    img.putalpha(mask)
    img.save(OUT / f"{name}.png", optimize=True)


hero()
architecture()
SPOTIFY = ("#ffffff", "#121212")
for capture in sorted(CAPTURES.glob("*.ansi")):
    colors = ("#cdd6f4", "#1e1e2e") if capture.stem.startswith("theme") else SPOTIFY
    terminal(capture.stem, *colors)
print("ok")
