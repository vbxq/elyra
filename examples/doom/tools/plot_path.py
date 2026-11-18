import struct
import sys

from PIL import Image, ImageDraw

WAD = "/home/vbxq/.local/share/elyra-runs/assets/fd/freedoom-0.13.0/freedoom1.wad"


def load_map(path):
    data = open(path, "rb").read()
    _, count, dirofs = struct.unpack_from("<4sii", data, 0)
    lumps = []
    for i in range(count):
        pos, size, raw = struct.unpack_from("<ii8s", data, dirofs + 16 * i)
        lumps.append((raw.split(b"\0")[0].decode("latin-1").upper(), pos, size))
    m = next(i for i, l in enumerate(lumps) if l[0] == "E1M1")

    def lump(name):
        for _, p, s in [l for l in lumps[m + 1:m + 11] if l[0] == name]:
            return data[p:p + s]

    verts = list(struct.iter_unpack("<hh", lump("VERTEXES")))
    lines = list(struct.iter_unpack("<HHHHHHH", lump("LINEDEFS")))
    sides = [s[5] for s in struct.iter_unpack("<hh8s8s8sh", lump("SIDEDEFS"))]
    sectors = list(struct.iter_unpack("<hh8s8shhh", lump("SECTORS")))
    things = list(struct.iter_unpack("<hhhhh", lump("THINGS")))
    return verts, lines, sides, sectors, things


SKILL = 2
MONSTERS = (3004, 9, 3001, 3002, 58, 3006, 3005, 3003, 16, 7)
RADIUS = {3004: 20, 9: 20, 3001: 20, 3002: 30, 58: 30, 3006: 16, 3005: 31, 3003: 24, 2035: 10, 54: 32, 70: 16}
for k in (2028, 30, 31, 32, 33, 35, 36, 37, 41, 42, 43, 44, 45, 46, 47, 48, 55, 56, 57, 25, 26, 27, 28, 29, 49, 50, 51, 52, 53):
    RADIUS[k] = 16


def main():
    out = sys.argv[1]
    box = [int(v) for v in sys.argv[2:6]] if len(sys.argv) >= 6 else [-720, -1080, 3260, 2350]
    scale = float(sys.argv[6]) if len(sys.argv) >= 7 else 0.5
    verts, lines, sides, sectors, things = load_map(WAD)
    x0, y0, x1, y1 = box
    img = Image.new("RGB", (int((x1 - x0) * scale) + 20, int((y1 - y0) * scale) + 20))
    d = ImageDraw.Draw(img)

    def p(x, y):
        return (10 + (x - x0) * scale, 10 + (y1 - y) * scale)

    for v1, v2, flags, special, _, front, back in lines:
        if back == 0xFFFF or flags & 1:
            c = (220, 220, 220)
        else:
            f, k = sectors[sides[front]], sectors[sides[back]]
            c = (200, 130, 0) if abs(f[0] - k[0]) > 24 else (80, 80, 80)
        if special:
            c = (255, 0, 0)
        d.line([p(*verts[v1]), p(*verts[v2])], fill=c)
    for x, y, _, kind, flags in things:
        r = RADIUS.get(kind, 0)
        if r and flags & SKILL and not flags & 16 and kind not in MONSTERS:
            d.rectangle([p(x - r, y + r), p(x + r, y - r)], outline=(60, 120, 255))
        if kind == 1:
            q = p(x, y)
            d.ellipse([q[0] - 4, q[1] - 4, q[0] + 4, q[1] + 4], outline=(0, 255, 0))
    pts = []
    for line in sys.stdin:
        f = line.split()
        if f and f[0] == "trace":
            pts.append((int(f[1]), int(f[2]), int(f[3])))
    for i in range(1, len(pts)):
        t = pts[i][0]
        c = (0, 255 - min(255, t // 10), min(255, t // 10))
        d.line([p(*pts[i - 1][1:]), p(*pts[i][1:])], fill=c, width=2)
    for i, (t, x, y) in enumerate(pts):
        if t % 350 == 0:
            q = p(x, y)
            d.ellipse([q[0] - 4, q[1] - 4, q[0] + 4, q[1] + 4], outline=(255, 255, 0))
            d.text((q[0] + 5, q[1]), str(t), fill=(255, 255, 0))
    img.save(out)


main()
