"""Renders the README images of spotify-terminal (PNG, drawn at 2x).

    python3 render.py OUT_DIR FONTS_DIR CAPTURES_DIR

Needs Pillow. FONTS_DIR holds Iosevka (https://github.com/be5invis/Iosevka) as
iosevka-regular.ttf, iosevka-bold.ttf, iosevka-italic.ttf; a Nerd Font Mono as
nerd.ttf, for the player's icons; and Doto (https://github.com/oliverlalan/Doto,
SIL OFL) as doto.ttf, for the title.

CAPTURES_DIR holds real terminal output: each NAME.ansi is one screen of `spt`
captured with `tmux capture-pane -e -p`, 150 columns wide. Album covers reach the
terminal as kitty Unicode placeholders, which a capture keeps as text; NAME.covers
lists, one per line, `RRGGBB IMAGE` for the placeholders of that color (the color
is the image's ID: see src/cover.rs), and the image is drawn in their place.

Style: monochrome ink on paper (or paper on ink), hairline rules, uppercase mono
labels, dot-matrix only for display, and Spotify's green as the one accent.
"""
import math
import re
import sys
import unicodedata
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT, FONTS, CAPTURES = (Path(arg) for arg in sys.argv[1:4])
K = 2  # supersampling: every logical pixel is K device pixels

THEMES = {
    "light": dict(bg="#ffffff", ink="#000000", muted="#585a5a", hair="#e5e7eb", off="#e9eaec", accent="#1aa34a"),
    "dark": dict(bg="#000000", ink="#ffffff", muted="#9a9c9c", hair="#2a2a2a", off="#1c1c1c", accent="#1ed760"),
}


def font(name, size):
    return ImageFont.truetype(str(FONTS / name), size * K)


def mono(size, bold=False):
    return font("iosevka-bold.ttf" if bold else "iosevka-regular.ttf", size)


def doto(size, weight=700, round_=100):
    face = font("doto.ttf", size)
    try:
        values = []
        for axis in face.get_variation_axes():
            name = axis["name"].decode() if isinstance(axis["name"], bytes) else axis["name"]
            values.append(weight if "eight" in name else round_)
        face.set_variation_by_axes(values)
    except OSError:
        pass
    return face


def spaced(draw, xy, text, face, fill, tracking=0.0):
    """Draws text with extra letter spacing (in logical px)."""
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=face, fill=fill)
        x += draw.textlength(ch, font=face) + tracking * K
    return x


def text_width(draw, text, face, tracking=0.0):
    return sum(draw.textlength(c, font=face) for c in text) + tracking * K * max(len(text) - 1, 0)


# --- Dot-matrix equalizer, the player's logo ---------------------------------------------
BARS = [9, 15, 22, 15, 9]  # LEDs lit in each bar, as assets/logo.txt draws it


def equalizer(img, x0, y0, cols, rows, pitch, theme):
    """Five bars of LEDs rising from the bottom; the top LED of each is the accent."""
    t = THEMES[theme]
    d = ImageDraw.Draw(img)
    width, gap = 4, 3
    left = (cols - (len(BARS) * width + (len(BARS) - 1) * gap)) // 2
    lit, tops = set(), set()
    for bar, height in enumerate(BARS):
        start = left + bar * (width + gap)
        for i in range(start, start + width):
            for j in range(rows - height, rows):
                lit.add((i, j))
            tops.add((i, rows - height))
    for j in range(rows):
        for i in range(cols):
            cx = (x0 + i * pitch + pitch / 2) * K
            cy = (y0 + j * pitch + pitch / 2) * K
            if (i, j) in tops:
                r, color = pitch * 0.36 * K, t["accent"]
            elif (i, j) in lit:
                r, color = pitch * 0.36 * K, t["ink"]
            else:
                r, color = pitch * 0.11 * K, t["off"]
            d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)


def hero(theme):
    t = THEMES[theme]
    W, H = 960, 400
    img = Image.new("RGB", (W * K, H * K), t["bg"])
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, W * K - 1, H * K - 1], outline=t["hair"], width=K)
    equalizer(img, 40, 40, 38, 32, 10, theme)

    x = 470
    cap = mono(11)
    chip = "UNOFFICIAL  ·  TERMINAL CLIENT"
    cwid = text_width(d, chip, cap, 0.6) / K
    d.rectangle([x * K, 58 * K, (x + cwid + 20) * K, 82 * K], outline=t["hair"], width=K)
    spaced(d, ((x + 10) * K, 63 * K), chip, cap, t["muted"], 0.6)

    title = "spotify-terminal"
    size = 54
    while size > 20 and text_width(d, title, doto(size), 1.0) / K > W - 40 - x:
        size -= 1
    spaced(d, (x * K, 110 * K), title, doto(size), t["ink"], 1.0)

    body = mono(17)
    d.text((x * K, 190 * K), "Your library, search and artists,", font=body, fill=t["ink"])
    d.text((x * K, 214 * K), "played from a terminal.", font=body, fill=t["ink"])

    specs = [("AUDIO", "LIBRESPOT · 320 KBIT/S"), ("INTERFACE", "HTML + CSS, COMPILED"), ("NEEDS", "SPOTIFY PREMIUM")]
    y = 270
    lab, val = mono(11), mono(13)
    for key, value in specs:
        d.line([(x * K, y * K), ((W - 40) * K, y * K)], fill=t["hair"], width=K)
        spaced(d, (x * K, (y + 10) * K), key, lab, t["muted"], 0.6)
        d.text(((x + 140) * K, (y + 8) * K), value, font=val, fill=t["ink"])
        y += 34
    d.line([(x * K, y * K), ((W - 40) * K, y * K)], fill=t["hair"], width=K)
    img.save(OUT / f"hero-{theme}.png", optimize=True)


# --- How it works ---------------------------------------------------------------------------
def dotted(d, x0, y0, x1, y1, color, pitch=7, r=1.6):
    n = max(1, int(math.hypot(x1 - x0, y1 - y0) / pitch))
    for i in range(n + 1):
        x = (x0 + (x1 - x0) * i / n) * K
        y = (y0 + (y1 - y0) * i / n) * K
        d.ellipse([x - r * K, y - r * K, x + r * K, y + r * K], fill=color)


def card(d, x, y, w, h, t, strong=False):
    d.rounded_rectangle([x * K, y * K, (x + w) * K, (y + h) * K], radius=8 * K, fill=t["bg"],
                        outline=t["ink"] if strong else t["hair"], width=K)


def architecture(theme):
    t = THEMES[theme]
    W, H = 960, 400
    img = Image.new("RGB", (W * K, H * K), t["bg"])
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, W * K - 1, H * K - 1], outline=t["hair"], width=K)
    lab, code = mono(11), mono(14)

    def label(x, y, s, color=None):
        spaced(d, (x * K, y * K), s, lab, color or t["muted"], 0.6)

    for x, n, s in [(40, "01", "YOUR TERMINAL"), (360, "02", "ONE PROCESS: spt"), (690, "03", "SPOTIFY")]:
        label(x, 36, n, t["ink"])
        label(x + 30, 36, s)

    # 01 the terminal: what the user sees and presses
    rows = [("ui/*.html + CSS", "COMPILED AT BUILD TIME"), ("keys, mouse", "REMAPPABLE IN [keys]"), ("kitty images", "ALBUM COVERS, ALSO IN TMUX")]
    for i, (c, s) in enumerate(rows):
        y = 66 + i * 76
        card(d, 40, y, 250, 60, t)
        d.text((56 * K, (y + 12) * K), c, font=code, fill=t["ink"])
        label(56, y + 36, s)

    # 02 the process
    card(d, 360, 66, 260, 212, t, strong=True)
    for i, line in enumerate(["hypercmd views", "Rust state", "librespot speaker", "MPRIS on D-Bus"]):
        d.text((378 * K, (84 + i * 26) * K), line, font=code, fill=t["ink"])
    d.line([(378 * K, 196 * K), (602 * K, 196 * K)], fill=t["hair"], width=K)
    label(378, 208, "READS ~/.config/spt/config.toml")
    label(378, 226, "REMEMBERS THE LAST VIEW")
    label(378, 244, "PLAYS THE AUDIO ITSELF")

    # 03 Spotify
    card(d, 690, 66, 230, 212, t)
    for i, line in enumerate(["accounts login", "catalog, library", "audio streams"]):
        d.text((706 * K, (84 + i * 26) * K), line, font=code, fill=t["ink"])
    d.line([(706 * K, 170 * K), (902 * K, 170 * K)], fill=t["hair"], width=K)
    label(706, 182, "OAUTH WITH PKCE, IN")
    label(706, 200, "YOUR BROWSER, ONCE")
    rx, ry = 706, 236
    d.ellipse([rx * K, (ry + 1) * K, (rx + 8) * K, (ry + 9) * K], fill=t["accent"])
    label(rx + 16, ry, "A CONNECT DEVICE")
    label(rx + 16, ry + 18, "OTHER APPS CAN DRIVE")

    for i in range(3):
        y = 96 + i * 76
        dotted(d, 296, y, 352, 172 if i == 1 else (120 if i == 0 else 224), t["ink"])
    dotted(d, 626, 172, 684, 172, t["ink"], pitch=6, r=2.2)

    y = 330
    dotted(d, 490, 284, 490, y, t["muted"], pitch=8, r=1.3)
    label(40, 350, "playerctl, MEDIA KEYS, STATUS BARS  →  MPRIS   ·   YOUR PHONE  →  SPOTIFY CONNECT")
    img.save(OUT / f"architecture-{theme}.png", optimize=True)


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
    img.save(OUT / f"{name}.png", optimize=True)


for th in THEMES:
    hero(th)
    architecture(th)
SPOTIFY = ("#ffffff", "#121212")
for capture in sorted(CAPTURES.glob("*.ansi")):
    colors = ("#cdd6f4", "#1e1e2e") if capture.stem.startswith("theme") else SPOTIFY
    terminal(capture.stem, *colors)
print("ok")
