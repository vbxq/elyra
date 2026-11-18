import struct
import sys

MOD = 1000000007


def mix(h, v):
    return (h * 31 + v) % MOD


def name_of(raw):
    raw = raw.split(b"\0", 1)[0]
    return raw.decode("latin-1").upper()


def name_hash(h, name):
    for ch in name.encode("latin-1"):
        h = mix(h, ch)
    return mix(h, 0)


class Wad:
    def __init__(self, path):
        with open(path, "rb") as f:
            self.data = f.read()
        ident, count, dirofs = struct.unpack_from("<4sii", self.data, 0)
        self.ident = ident.decode("latin-1")
        self.lumps = []
        for i in range(count):
            pos, size, raw = struct.unpack_from("<ii8s", self.data, dirofs + 16 * i)
            self.lumps.append((name_of(raw), pos, size))

    def find(self, name):
        for i in range(len(self.lumps) - 1, -1, -1):
            if self.lumps[i][0] == name:
                return i
        return -1

    def find_after(self, name, start):
        for i in range(start, len(self.lumps)):
            if self.lumps[i][0] == name:
                return i
        return -1

    def lump(self, i):
        _, pos, size = self.lumps[i]
        return self.data[pos:pos + size]


def map_summary(wad, out):
    m = wad.find("E1M1")
    names = ["THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS",
             "SSECTORS", "NODES", "SECTORS", "REJECT", "BLOCKMAP"]
    lumps = {n: wad.lump(wad.find_after(n, m + 1)) for n in names}

    things = list(struct.iter_unpack("<hhhhh", lumps["THINGS"]))
    h = 0
    for t in things:
        for v in t:
            h = mix(h, v)
    out.append(f"things {len(things)} hash {h}")
    p1 = next(t for t in things if t[3] == 1)
    out.append(f"player1 x {p1[0]} y {p1[1]} angle {p1[2]}")

    lines = list(struct.iter_unpack("<HHHHHHH", lumps["LINEDEFS"]))
    h = 0
    twosided = 0
    for ld in lines:
        v1, v2, flags, special, tag, front, back = ld
        front = -1 if front == 0xFFFF else front
        back = -1 if back == 0xFFFF else back
        if back >= 0:
            twosided += 1
        for v in (v1, v2, flags, special, tag, front, back):
            h = mix(h, v)
    out.append(f"linedefs {len(lines)} twosided {twosided} hash {h}")

    sides = list(struct.iter_unpack("<hh8s8s8sh", lumps["SIDEDEFS"]))
    h = 0
    for xo, yo, up, lo, mid, sec in sides:
        h = mix(h, xo)
        h = mix(h, yo)
        h = name_hash(h, name_of(up))
        h = name_hash(h, name_of(lo))
        h = name_hash(h, name_of(mid))
        h = mix(h, sec)
    out.append(f"sidedefs {len(sides)} hash {h}")

    verts = list(struct.iter_unpack("<hh", lumps["VERTEXES"]))
    h = 0
    for x, y in verts:
        h = mix(h, x)
        h = mix(h, y)
    out.append(f"vertexes {len(verts)} hash {h}")

    segs = list(struct.iter_unpack("<HHhHhh", lumps["SEGS"]))
    h = 0
    for s in segs:
        for v in s:
            h = mix(h, v)
    out.append(f"segs {len(segs)} hash {h}")

    ss = list(struct.iter_unpack("<HH", lumps["SSECTORS"]))
    h = 0
    for c, f in ss:
        h = mix(h, c)
        h = mix(h, f)
    out.append(f"ssectors {len(ss)} hash {h}")

    nodes = list(struct.iter_unpack("<hhhh4h4hHH", lumps["NODES"]))
    h = 0
    leaves = 0
    for n in nodes:
        for v in n[:12]:
            h = mix(h, v)
        for child in n[12:]:
            if child & 0x8000:
                leaves += 1
                h = mix(h, 1)
                h = mix(h, child & 0x7FFF)
            else:
                h = mix(h, 0)
                h = mix(h, child)
    out.append(f"nodes {len(nodes)} leaves {leaves} hash {h}")

    secs = list(struct.iter_unpack("<hh8s8shhh", lumps["SECTORS"]))
    h = 0
    for fh, ch, ff, cf, light, special, tag in secs:
        h = mix(h, fh)
        h = mix(h, ch)
        h = name_hash(h, name_of(ff))
        h = name_hash(h, name_of(cf))
        h = mix(h, light)
        h = mix(h, special)
        h = mix(h, tag)
    out.append(f"sectors {len(secs)} hash {h}")

    out.append(f"reject bytes {len(lumps['REJECT'])}")

    bm = lumps["BLOCKMAP"]
    ox, oy, cols, rows = struct.unpack_from("<hhhh", bm, 0)
    nblocks = cols * rows
    h = 0
    entries = 0
    for b in range(nblocks):
        off = struct.unpack_from("<H", bm, 8 + 2 * b)[0]
        p = off * 2
        while True:
            v = struct.unpack_from("<H", bm, p)[0]
            p += 2
            if v == 0xFFFF:
                break
            entries += 1
            h = mix(h, v)
        h = mix(h, b)
    out.append(f"blockmap origin {ox} {oy} size {cols}x{rows} blocks {nblocks} entries {entries} hash {h}")


def decode_patch(lump):
    width, height, left, top = struct.unpack_from("<HHhh", lump, 0)
    cols = []
    for x in range(width):
        ofs = struct.unpack_from("<I", lump, 8 + 4 * x)[0]
        posts = []
        prev = -1
        while lump[ofs] != 0xFF:
            delta = lump[ofs]
            length = lump[ofs + 1]
            topd = prev + delta if delta <= prev else delta
            prev = topd
            posts.append((topd, lump[ofs + 3:ofs + 3 + length]))
            ofs += length + 4
        cols.append(posts)
    return width, height, cols


def gfx_summary(wad, out):
    pal = wad.lump(wad.find("PLAYPAL"))
    h = 0
    for b in pal:
        h = mix(h, b)
    out.append(f"playpal palettes {len(pal) // 768} hash {h}")
    cm = wad.lump(wad.find("COLORMAP"))
    h = 0
    for b in cm:
        h = mix(h, b)
    out.append(f"colormap maps {len(cm) // 256} hash {h}")

    pn = wad.lump(wad.find("PNAMES"))
    npn = struct.unpack_from("<i", pn, 0)[0]
    pnames = [name_of(pn[4 + 8 * i:12 + 8 * i]) for i in range(npn)]
    patch_lumps = [wad.find(n) for n in pnames]
    missing = sum(1 for p in patch_lumps if p < 0)
    out.append(f"pnames {npn} missing {missing}")

    textures = []
    for tname in ("TEXTURE1", "TEXTURE2"):
        li = wad.find(tname)
        if li < 0:
            continue
        t = wad.lump(li)
        n = struct.unpack_from("<i", t, 0)[0]
        for i in range(n):
            ofs = struct.unpack_from("<i", t, 4 + 4 * i)[0]
            name = name_of(t[ofs:ofs + 8])
            w, hh = struct.unpack_from("<hh", t, ofs + 12)
            pc = struct.unpack_from("<h", t, ofs + 20)[0]
            patches = []
            for p in range(pc):
                ox, oy, idx = struct.unpack_from("<hhh", t, ofs + 22 + 10 * p)
                patches.append((ox, oy, idx))
            textures.append((name, w, hh, patches))
    total_patches = sum(len(t[3]) for t in textures)
    out.append(f"textures {len(textures)} patchrefs {total_patches}")

    def compose(tex):
        name, w, hh, patches = tex
        pix = bytearray(w * hh)
        for ox, oy, idx in patches:
            li = patch_lumps[idx]
            if li < 0:
                continue
            pw, ph, cols = decode_patch(wad.lump(li))
            for x in range(pw):
                tx = ox + x
                if tx < 0 or tx >= w:
                    continue
                for topd, data in cols[x]:
                    for i, c in enumerate(data):
                        ty = oy + topd + i
                        if 0 <= ty < hh:
                            pix[tx * hh + ty] = c
        return pix

    t0 = textures[0]
    pix = compose(t0)
    h = 0
    for b in pix:
        h = mix(h, b)
    out.append(f"texture0 {t0[0]} {t0[1]}x{t0[2]} patches {len(t0[3])} hash {h}")

    h = 0
    for tex in textures:
        h = mix(h, len(tex[0]))
        for b in compose(tex):
            h = mix(h, b)
    out.append(f"alltextures hash {h}")

    fs = wad.find("F_START")
    fe = wad.find("F_END")
    flats = [i for i in range(fs + 1, fe) if wad.lumps[i][2] == 4096]
    f0 = flats[0]
    h = 0
    for b in wad.lump(f0):
        h = mix(h, b)
    out.append(f"flats {len(flats)} flat0 {wad.lumps[f0][0]} hash {h}")


def main():
    wad = Wad(sys.argv[1])
    out = [f"wad {wad.ident} lumps {len(wad.lumps)}"]
    map_summary(wad, out)
    gfx_summary(wad, out)
    print("\n".join(out))


main()
