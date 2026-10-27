"""Renders the ClipPro app icon (1024×1024 PNG) without external libraries.

Dark rounded square with three accent bars of rising height (a score bar / selects motif).
Run: python app/design/make_icon.py  →  app/design/app-icon.png
"""
import math
import struct
import zlib
from pathlib import Path

N = 1024
SS = 3  # supersampling per axis


def sd_round_rect(px, py, cx, cy, hw, hh, r):
    qx = abs(px - cx) - (hw - r)
    qy = abs(py - cy) - (hh - r)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    inside = min(max(qx, qy), 0.0)
    return outside + inside - r


def colour_at(x, y):
    # Background squircle-ish rounded square with a subtle vertical gradient.
    d = sd_round_rect(x, y, 512, 512, 432, 432, 190)
    if d > 0:
        return None
    t = y / N
    bg = (int(44 - 14 * t), int(44 - 14 * t), int(48 - 14 * t))
    # Three bars, rising left to right, accent blue with a lighter top.
    bars = [(352, 610, 70), (512, 520, 70), (672, 410, 70)]
    for cx, top, hw in bars:
        bottom = 700
        cy = (top + bottom) / 2
        hh = (bottom - top) / 2
        if sd_round_rect(x, y, cx, cy, hw, hh, 34) <= 0:
            k = (y - top) / max(1, bottom - top)
            return (int(64 - 54 * k), int(160 - 28 * k), int(255))
    # Baseline hairline.
    if 744 <= y <= 756 and 270 <= x <= 754:
        return (90, 90, 96)
    return bg


def render():
    rows = []
    for j in range(N):
        row = bytearray([0])  # filter type 0
        for i in range(N):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sj in range(SS):
                for si in range(SS):
                    c = colour_at(i + (si + 0.5) / SS, j + (sj + 0.5) / SS)
                    if c is not None:
                        acc[0] += c[0]
                        acc[1] += c[1]
                        acc[2] += c[2]
                        acc[3] += 255
            n = SS * SS
            a = acc[3] / n
            if a > 0:
                cov = acc[3] / 255
                row += bytes([int(acc[0] / cov), int(acc[1] / cov), int(acc[2] / cov), int(a)])
            else:
                row += bytes([0, 0, 0, 0])
        rows.append(bytes(row))
    return b"".join(rows)


def png(data, path):
    def chunk(tag, body):
        c = struct.pack(">I", len(body)) + tag + body
        return c + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0)
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
    Path(path).write_bytes(out)


if __name__ == "__main__":
    here = Path(__file__).parent
    png(render(), here / "app-icon.png")
    print("wrote", here / "app-icon.png")
