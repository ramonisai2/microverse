"""Export procedural hero → assets/entities/hero.json (classic palette indices)."""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def rgb(c):
    return ((c >> 16) & 255) / 255, ((c >> 8) & 255) / 255, (c & 255) / 255


SKIN, SHIRT, PANTS, BOOTS, METAL, GOLD, CAPE, SWORD, EYE, MOUTH, BELT = (
    0xFDBF8A,
    0xE67E22,
    0x2C3E50,
    0x8B4513,
    0xC0C0C0,
    0xF1C40F,
    0xE74C3C,
    0xECF0F1,
    0x111111,
    0x8B0000,
    0x4A2E1B,
)


def voxel_color(x, y, z):
    if abs(x) <= 5 and abs(z) <= 3 and 17 <= y <= 24:
        return rgb(SHIRT)
    if abs(x) == 6 and abs(z) <= 4 and 23 <= y <= 24:
        return rgb(METAL)
    if abs(z) == 4 and abs(x) <= 5 and 23 <= y <= 24:
        return rgb(METAL)
    if abs(x) <= 5 and abs(z) <= 3 and y == 16:
        return rgb(BELT)
    if x == 0 and z == -4 and y == 16:
        return rgb(GOLD)
    if -8 <= x <= -5 and abs(z) <= 2:
        if 19 <= y <= 23:
            return rgb(SHIRT)
        if 16 <= y <= 18:
            return rgb(SKIN)
    if 5 <= x <= 8 and abs(z) <= 2:
        if 19 <= y <= 23:
            return rgb(SHIRT)
        if 16 <= y <= 18:
            return rgb(SKIN)
    if -4 <= x <= 4 and abs(z) <= 2 and 8 <= y <= 15:
        return rgb(PANTS)
    if -4 <= x <= 4 and abs(z) <= 2 and 6 <= y <= 7:
        return rgb(BOOTS)
    if abs(x) <= 4 and 4 <= z <= 6 and 18 <= y <= 22:
        return rgb(CAPE)
    if -6 <= x <= -4 and 2 <= z <= 3 and 14 <= y <= 20:
        return rgb(SWORD)
    if x == -5 and z == 2 and y in (12, 13, 17):
        return rgb(GOLD)
    if abs(x) <= 2 and abs(z) <= 2 and 24 <= y <= 26:
        return rgb(SKIN)
    hx, hy, hz = float(x), float(y - 27), float(z)
    if math.sqrt(hx * hx + hy * hy + hz * hz) <= 4.5:
        return rgb(SKIN)
    hely = float(y - 30)
    if math.sqrt(hx * hx + hely * hely + hz * hz) <= 5.5 and y > 27:
        return rgb(METAL)
    if abs(x) == 2 and y == 28 and z == -5:
        return rgb(EYE)
    if abs(x) <= 1 and y == 26 and z == -5:
        return rgb(MOUTH)
    return None


classic = json.loads((ROOT / "assets/palettes/classic.json").read_text())


def hex_rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i : i + 2], 16) / 255 for i in (0, 2, 4))


pal = [hex_rgb(h) for h in classic]


def nearest(c):
    best, bd = 0, 1e9
    for i, p in enumerate(pal):
        d = sum((a - b) ** 2 for a, b in zip(c, p))
        if d < bd:
            bd, best = d, i
    return best


def main():
    voxels = []
    for x in range(-7, 9):
        for y in range(0, 32):
            for z in range(-7, 9):
                c = voxel_color(x, y, z)
                if c:
                    voxels.append({"x": x, "y": y, "z": z, "c": nearest(c)})
    out = {
        "id": "hero",
        "grid": {"x": 16, "y": 32, "z": 16},
        "foot_y": 6,
        "palette": "classic",
        "voxels": voxels,
    }
    path = ROOT / "assets/entities/hero.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(out, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {path} ({len(voxels)} voxels)")


if __name__ == "__main__":
    main()
