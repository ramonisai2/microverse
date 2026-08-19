"""Generate assets/grass.png — RGB, magenta chroma key (no alpha)."""
import struct
import zlib
from pathlib import Path

W, H = 32, 28
MAGENTA = (255, 0, 255)
GREEN = (66, 140, 70)
DARK = (40, 90, 45)
LIGHT = (110, 180, 90)


def pix(x: int, y: int) -> tuple[int, int, int]:
    fy = (H - 1 - y) / (H - 1)
    fx = (x + 0.5) / W
    blades = [
        (0.22, 0.95, 0.06),
        (0.38, 1.00, 0.07),
        (0.52, 0.88, 0.05),
        (0.68, 0.98, 0.07),
        (0.82, 0.90, 0.06),
    ]
    for cx, h, half_w in blades:
        if fy > h:
            continue
        t = fy / h
        w = half_w * (1.0 - t * 0.75)
        lean = (t * t) * 0.03 * (1.0 if cx < 0.5 else -1.0)
        if abs(fx - (cx + lean)) <= w:
            if t > 0.7:
                return LIGHT
            if (x + y) % 5 == 0:
                return DARK
            return GREEN
    return MAGENTA


def chunk(tag: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(tag + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)


def main() -> None:
    rows = []
    for y in range(H):
        row = b"\x00"
        for x in range(W):
            r, g, b = pix(x, y)
            row += bytes((r, g, b))
        rows.append(row)
    raw = b"".join(rows)
    comp = zlib.compress(raw, 9)
    ihdr = struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)  # 8-bit RGB
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", comp)
        + chunk(b"IEND", b"")
    )
    out = Path(__file__).resolve().parents[1] / "assets" / "grass.png"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png)
    print(f"wrote {out} ({len(png)} bytes)")


if __name__ == "__main__":
    main()
