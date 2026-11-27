"""Draws the Sözlük icon: a rounded tile in TDK's red (sozluk.gov.tr's #c62828 to #a93226), an open white book, and over
its spine the breve of ğ. Writes assets/icon-1024.png, assets/icon-256.png, assets/icon.ico.

usage: python -I tools/make_icon.py        (needs Pillow)
The same geometry is in assets/logo.svg (by hand): change both together.
"""
import os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'assets')
S = 4                     # supersampling
N = 1024 * S

def bez(p0, p1, p2, steps=64):
    """Points of a quadratic Bézier curve."""
    out = []
    for i in range(steps + 1):
        t = i / steps
        x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t * t * p2[0]
        y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t * t * p2[1]
        out.append((x * S, y * S))
    return out

def tile():
    # Diagonal gradient #d8443a (top left) to #a02a20 (bottom right), TDK's reds, in a rounded square.
    grad = Image.new('RGB', (N, N))
    a, b = (0xd8, 0x44, 0x3a), (0xa0, 0x2a, 0x20)
    px = grad.load()
    step = 8
    for y in range(0, N, step):
        for x in range(0, N, step):
            t = (x + y) / (2 * N)
            c = tuple(round(a[k] + (b[k] - a[k]) * t) for k in range(3))
            for yy in range(y, min(y + step, N)):
                for xx in range(x, min(x + step, N)):
                    px[xx, yy] = c
    mask = Image.new('L', (N, N), 0)
    ImageDraw.Draw(mask).rounded_rectangle((64 * S, 64 * S, 960 * S, 960 * S), radius=200 * S, fill=255)
    img = Image.new('RGBA', (N, N), (0, 0, 0, 0))
    img.paste(grad, (0, 0), mask)
    return img

def book(d):
    white = (255, 255, 255, 255)
    # Left page: spine (500, 470) to (500, 790); outer edge at x = 236; top and bottom curve.
    left = bez((500, 470), (380, 400), (236, 430)) + bez((236, 430), (236, 600), (236, 760)) \
        + bez((236, 760), (380, 730), (500, 790))
    right = [(2 * 512 * S - x, y) for (x, y) in left]
    d.polygon(left, fill=white)
    d.polygon(right, fill=white)
    # Lines of text on the pages.
    ink = (0xf2, 0xb8, 0xb2, 255)
    for k, y in enumerate((520, 585, 650)):
        w = 150 if k < 2 else 110
        for side in (-1, 1):
            x0 = 512 + side * 60
            x1 = x0 + side * w
            lo, hi = sorted((x0, x1))
            d.rounded_rectangle((lo * S, (y - 2 + (0 if side < 0 else 0)) * S, hi * S, (y + 20) * S), radius=11 * S, fill=ink)

def breve(d):
    # The breve of ğ: one even stroke with round ends along a curve over the spine.
    r = 34
    for (x, y) in bez((418, 236), (512, 410), (606, 236), 400):
        d.ellipse((x - r * S, y - r * S, x + r * S, y + r * S), fill=(255, 255, 255, 255))

def main():
    os.makedirs(OUT, exist_ok=True)
    img = tile()
    d = ImageDraw.Draw(img)
    book(d)
    breve(d)
    big = img.resize((1024, 1024), Image.LANCZOS)
    big.save(os.path.join(OUT, 'icon-1024.png'))
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, 'icon-256.png'))
    big.save(os.path.join(OUT, 'icon.ico'), sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)])
    # Raw RGBA for the window and taskbar icons (winit takes pixels, not PNG).
    for n in (32, 256):
        with open(os.path.join(OUT, f'icon-{n}.rgba'), 'wb') as f:
            f.write(big.resize((n, n), Image.LANCZOS).convert('RGBA').tobytes())
    print('wrote assets/icon-1024.png, icon-256.png, icon.ico, icon-32.rgba, icon-256.rgba')

if __name__ == '__main__':
    main()
