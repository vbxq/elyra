import struct
import sys
import zlib


def read_ppm(path):
    data = open(path, "rb").read()
    tokens = []
    pos = 0
    while len(tokens) < 4:
        while data[pos:pos + 1].isspace():
            pos += 1
        if data[pos:pos + 1] == b"#":
            while data[pos:pos + 1] not in (b"\n", b""):
                pos += 1
            continue
        start = pos
        while not data[pos:pos + 1].isspace():
            pos += 1
        tokens.append(data[start:pos])
    magic, w, h, maxval = tokens[0], int(tokens[1]), int(tokens[2]), int(tokens[3])
    if magic == b"P6":
        raw = data[pos + 1:pos + 1 + w * h * 3]
    elif magic == b"P3":
        raw = bytes(int(v) for v in data[pos:].split()[:w * h * 3])
    else:
        sys.exit(f"{path}: unsupported {magic!r}")
    if maxval != 255:
        raw = bytes(v * 255 // maxval for v in raw)
    return w, h, raw


def write_png(path, w, h, raw, scale):
    rows = []
    for y in range(h):
        line = raw[y * w * 3:(y + 1) * w * 3]
        if scale > 1:
            line = b"".join(line[x * 3:x * 3 + 3] * scale for x in range(w))
        rows.extend([b"\x00" + line] * scale)

    def chunk(tag, body):
        c = struct.pack(">I", len(body)) + tag + body
        return c + struct.pack(">I", zlib.crc32(tag + body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w * scale, h * scale, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(b"".join(rows), 6))
    png += chunk(b"IEND", b"")
    open(path, "wb").write(png)


def main():
    scale = 2
    for path in sys.argv[1:]:
        if path.startswith("--scale="):
            scale = int(path.split("=")[1])
            continue
        w, h, raw = read_ppm(path)
        out = path.rsplit(".", 1)[0] + ".png"
        write_png(out, w, h, raw, scale)
        p6 = path.rsplit(".", 1)[0] + ".p6.ppm"
        open(p6, "wb").write(b"P6\n%d %d\n255\n" % (w, h) + raw)
        print(out)


main()
