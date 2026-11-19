DESCRIPTION = "match on char ranges, or-patterns, int ranges and guards inside tuples, Option and enum payloads, exhaustive or with a wildcard"

CMAX = 0x10FFFF
IMIN, IMAX = -(1 << 63), (1 << 63) - 1
ENDS = [ord(c) for c in "afmnzAMZ059"] + [0xE9, 0x7E]
INPUT_CH = [ord(c) for c in "amzAMZ09 ~fn5"] + [0xE9, 0, 0x10FFFF, 0x41, 0x7A, 0x6E, 0x60]
INPUT_INT = [0, 1, -1, 5, 7, 100, -100, 42]
IENDS = [-5, 0, 3, 7, 10, 50]
SHAPES = ["tup", "opt", "enum", "opttup"]
TYPES = {"tup": "(char, char)", "opt": "Option<char>", "enum": "K", "opttup": "Option<(char, int)>"}


def cl(c):
    if c == 0:
        return "'\\0'"
    if c < 0x20 or c > 0x7E or c in (0x27, 0x5C):
        return "'\\u{%x}'" % c
    return "'%s'" % chr(c)


def cmod(a, b):
    r = abs(a) % b
    return -r if a < 0 else r


def inside(v, ivs):
    return any(lo <= v <= hi for lo, hi in ivs)


def char_pat(rng):
    k = rng.random()
    if k < 0.15:
        return "_", [(0, CMAX)], False
    if k < 0.3:
        return None, [(0, CMAX)], True

    def one():
        kk = rng.random()
        a, b = sorted(rng.sample(ENDS, 2))
        if kk < 0.25:
            x = rng.choice(ENDS)
            return cl(x), [(x, x)]
        if kk < 0.55:
            return f"{cl(a)}..={cl(b)}", [(a, b)]
        if kk < 0.75:
            return f"{cl(a)}..{cl(b)}", [(a, b - 1)]
        if kk < 0.87:
            return f"..={cl(b)}", [(0, b)]
        return f"{cl(a)}..", [(a, CMAX)]

    s, iv = one()
    if rng.random() < 0.3:
        s2, iv2 = one()
        s, iv = s + " | " + s2, iv + iv2
    return s, iv, False


def int_pat(rng):
    k = rng.random()
    if k < 0.3:
        return "_", [(IMIN, IMAX)], False
    if k < 0.55:
        return "n", [(IMIN, IMAX)], True
    a, b = sorted(rng.sample(IENDS, 2))
    if k < 0.75:
        x = rng.choice(IENDS)
        return str(x), [(x, x)], False
    if k < 0.9:
        return f"{a}..={b}", [(a, b)], False
    return f"{a}..", [(a, IMAX)], False


def cuts(ivs_list, lo, hi):
    pts = {lo, hi + 1}
    for ivs in ivs_list:
        for a, b in ivs:
            pts.add(a)
            pts.add(b + 1)
    pts = sorted(p for p in pts if lo <= p <= hi + 1)
    reps = []
    for x, y in zip(pts, pts[1:]):
        if 0xD800 <= x and y - 1 <= 0xDFFF and hi == CMAX:
            continue
        reps.append(x if not (0xD800 <= x <= 0xDFFF) else 0xE000)
    return reps


class Arm:
    def __init__(self, kind, src, ivs, binds):
        # binds: (name, index into the value tuple)
        self.kind, self.src, self.ivs, self.binds = kind, src, ivs, binds

    def matches(self, v):
        k = self.kind
        if k == "tup":
            return inside(v[0], self.ivs[0]) and inside(v[1], self.ivs[1])
        if k == "none":
            return v is None
        if k in ("some", "A"):
            return v is not None and v[0] == ("S" if k == "some" else "A") and inside(v[1], self.ivs[0])
        if k in ("B", "st"):
            return v is not None and v[0] == ("B" if k == "B" else "S") and inside(v[1], self.ivs[0]) and inside(v[2], self.ivs[1])
        return v is not None and v[0] == "C"


def char_slot(rng, name):
    s, iv, binds = char_pat(rng)
    return (name if binds else s), iv, ([name] if binds else [])


def int_slot(rng):
    s, iv, binds = int_pat(rng)
    return s, iv, (["n"] if binds else [])


def make_arm(rng, shape):
    if shape == "tup":
        p1, i1, b1 = char_slot(rng, "c")
        p2, i2, b2 = char_slot(rng, "d" if b1 else "c")
        return Arm("tup", f"({p1}, {p2})", (i1, i2), [(n, 0) for n in b1] + [(n, 1) for n in b2])
    if shape == "opt":
        if rng.random() < 0.2:
            return Arm("none", "None", None, [])
        p, i, b = char_slot(rng, "c")
        return Arm("some", f"Some({p})", (i,), [(n, 1) for n in b])
    if shape == "enum":
        k = rng.random()
        if k < 0.4:
            p, i, b = char_slot(rng, "c")
            return Arm("A", f"K::A({p})", (i,), [(n, 1) for n in b])
        if k < 0.85:
            p, i, b = char_slot(rng, "c")
            q, j, bn = int_slot(rng)
            return Arm("B", f"K::B({p}, {q})", (i, j), [(n, 1) for n in b] + [(n, 2) for n in bn])
        return Arm("C", "K::C", None, [])
    if rng.random() < 0.2:
        return Arm("none", "None", None, [])
    p, i, b = char_slot(rng, "c")
    q, j, bn = int_slot(rng)
    return Arm("st", f"Some(({p}, {q}))", (i, j), [(n, 1) for n in b] + [(n, 2) for n in bn])


def guard(rng, name):
    if name in ("c", "d"):
        return rng.choice([
            (f"{name} > 'k'", lambda env: env[name] > ord("k")),
            (f"{name}.is_alpha()", lambda env: chr(env[name]).isalpha()),
            (f"({name} as int) % 2 == 0", lambda env: env[name] % 2 == 0),
        ])
    return rng.choice([
        (f"{name} % 2 == 0", lambda env: env[name] % 2 == 0),
        (f"{name} > 3", lambda env: env[name] > 3),
    ])


def classes(shape, arms):
    civs, iivs = [], []
    for a in arms:
        if a.kind == "tup":
            civs += [a.ivs[0], a.ivs[1]]
        elif a.kind in ("some", "A"):
            civs.append(a.ivs[0])
        elif a.kind in ("B", "st"):
            civs.append(a.ivs[0])
            iivs.append(a.ivs[1])
    cr, ir = cuts(civs, 0, CMAX), cuts(iivs, IMIN, IMAX)
    if shape == "tup":
        return [(a, b) for a in cr for b in cr]
    if shape == "opt":
        return [None] + [("S", a) for a in cr]
    if shape == "enum":
        return [("A", a) for a in cr] + [("B", a, n) for a in cr for n in ir] + [("C",)]
    return [None] + [("S", a, n) for a in cr for n in ir]


def random_input(rng, shape):
    c1, c2, n = rng.choice(INPUT_CH), rng.choice(INPUT_CH), rng.choice(INPUT_INT)
    if shape == "tup":
        return (c1, c2), f"({cl(c1)}, {cl(c2)})"
    if shape == "opt":
        return (None, "None") if rng.random() < 0.15 else (("S", c1), f"Some({cl(c1)})")
    if shape == "enum":
        k = rng.random()
        if k < 0.4:
            return ("A", c1), f"K::A({cl(c1)})"
        if k < 0.85:
            return ("B", c1, n), f"K::B({cl(c1)}, {n})"
        return ("C",), "K::C"
    return (None, "None") if rng.random() < 0.15 else (("S", c1, n), f"Some(({cl(c1)}, {n}))")


def generate(rng, index):
    shape = SHAPES[index % len(SHAPES)]
    raw = [make_arm(rng, shape) for _ in range(rng.randint(2, 6))]
    cls = classes(shape, raw)
    covered, arms = set(), []
    for a in raw:
        g = guard(rng, a.binds[0][0]) if a.binds and rng.random() < 0.6 else None
        mine = {i for i, v in enumerate(cls) if a.matches(v)}
        # an arm adding no new value class would be unreachable
        if not (mine - covered):
            continue
        arms.append((a, g))
        if g is None:
            covered |= mine
    lines = [f"fn classify(x: {TYPES[shape]}) -> int {{", "    match x {"]
    for i, (a, g) in enumerate(arms):
        body = f"{(i + 1) * 1000}" + "".join(
            f" + ({n} as int) % 97" if n in ("c", "d") else f" + {n} % 97" for n, _ in a.binds)
        lines.append(f"        {a.src}" + (f" if {g[0]}" if g else "") + f" => {body},")
    if len(covered) < len(cls):
        lines.append("        _ => 0,")
    lines += ["    }", "}"]
    inputs = [random_input(rng, shape) for _ in range(12)]
    out = []
    for v, _ in inputs:
        res = 0
        for i, (a, g) in enumerate(arms):
            if a.matches(v):
                env = {n: v[pos] for n, pos in a.binds}
                if g is None or g[1](env):
                    res = (i + 1) * 1000 + sum(cmod(env[n], 97) for n, _ in a.binds)
                    break
        out.append(f"{res}\n")
    pre = "enum K { A(char), B(char, int), C }\n" if shape == "enum" else ""
    src = pre + "\n".join(lines) + "\n" + "".join(f"println(classify({s}))\n" for _, s in inputs)
    return src, "".join(out)
