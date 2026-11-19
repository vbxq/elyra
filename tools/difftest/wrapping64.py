from common import TRAP, Trap, tdiv, as_main

DESCRIPTION = "wrapping u64/i64 expressions, shifts with out-of-range counts, casts and formatting, through containers"

M = 1 << 64


def u(v):
    return v % M


def s(v):
    v %= M
    return v - M if v >= 1 << 63 else v


WRAP = {"u64": u, "i64": s}
VALS = [0, 1, 2, 3, 2**47 - 1, 2**47, 2**48, 2**63 - 1, 2**63, 2**64 - 1, 2**32, 2**32 - 1, 0x9E3779B97F4A7C15,
        0xFF51AFD7ED558CCD, 0xC4CEB9FE1A85EC53, 0x100000001B3, 0xCBF29CE484222325, 12345]


def lit(ty, v):
    v = WRAP[ty](v)
    if v < 0:
        if v == -2**63:
            return "(-9223372036854775807 - 1)"
        return f"(0 - {-v})"
    return str(v)


def expr(rng, ty, names, d):
    w = WRAP[ty]
    if d == 0 or rng.random() < 0.3:
        if names and rng.random() < 0.6:
            n = rng.choice(names)
            return n, (lambda e, n=n: e[n])
        v = rng.choice(VALS + [rng.getrandbits(64), rng.randint(0, 100)])
        return lit(ty, v), (lambda e, v=w(v): v)
    op = rng.choice(["+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>", "neg", "not", "*", "+", "rotl"])
    a, fa = expr(rng, ty, names, d - 1)
    if op == "neg":
        if a[0].isdigit() or a.startswith("(0 - ") or a.startswith("(-9"):
            return f"(0 - {a})", (lambda e: w(-fa(e)))
        return f"(-{a})", (lambda e: w(-fa(e)))
    if op == "not":
        return f"(~{a})", (lambda e: w(~fa(e)))
    if op in ("<<", ">>"):
        k = rng.choice([0, 1, 7, 13, 31, 32, 33, 47, 63, 64, 65, -1])
        km = k & 63
        ks = f"({k})" if k < 0 else str(k)
        if op == "<<":
            return f"({a} << {ks})", (lambda e: w(fa(e) << km))
        if ty == "u64":
            return f"({a} >> {ks})", (lambda e: u(fa(e)) >> km)
        return f"({a} >> {ks})", (lambda e: s(fa(e)) >> km)
    if op == "rotl":
        k = rng.randint(1, 63)
        if ty == "u64":
            return f"(({a} << {k}) | ({a} >> {64 - k}))", (lambda e: u((fa(e) << k) | (u(fa(e)) >> (64 - k))))
        return f"({a} << {k})", (lambda e: w(fa(e) << k))
    b, fb = expr(rng, ty, names, d - 1)
    if op in ("/", "%"):
        def dv(x, y):
            if ty == "u64":
                x, y = u(x), u(y)
                if y == 0:
                    raise Trap()
                return x // y if op == "/" else x % y
            x, y = s(x), s(y)
            q = tdiv(x, y)
            return s(q) if op == "/" else s(x - y * q)
        return f"({a} {op} {b})", (lambda e: dv(fa(e), fb(e)))
    f = {"+": lambda x, y: x + y, "-": lambda x, y: x - y, "*": lambda x, y: x * y, "&": lambda x, y: x & y,
         "|": lambda x, y: x | y, "^": lambda x, y: x ^ y}[op]
    return f"({a} {op} {b})", (lambda e: w(f(fa(e), fb(e))))


ROUTES = ["plain", "struct", "vec", "map_key", "array", "closure", "tuple", "generic", "loop"]
HEAD = "struct B64 { a: u64, b: i64 }\nfn gid<T>(x: T) -> T { x }\n"


def route(kind, ty, i, src):
    n = f"r{i}"
    if kind == "plain":
        return f"let {n}: {ty} = {src}"
    if kind == "struct":
        fld = "a" if ty == "u64" else "b"
        return f"let mut s{i} = B64 {{ a: 0, b: 0 }}\ns{i}.{fld} = {src}\nlet {n} = s{i}.{fld}"
    if kind == "vec":
        return f"let mut v{i}: Vec<{ty}> = vec![1, 2]\nv{i}.push({src})\nv{i}[0] = v{i}[2]\nlet {n} = v{i}[0]"
    if kind == "array":
        return f"let mut a{i}: [{ty}; 2] = [0, 0]\na{i}[1] = {src}\nlet {n} = a{i}[1]"
    if kind == "map_key":
        return f"let mut m{i}: Map<{ty}, int> = Map::new()\nlet k{i}: {ty} = {src}\nm{i}[k{i}] = 5\nlet {n} = m{i}.keys()[0]"
    if kind == "closure":
        return f"let c{i} = fn() -> {ty} {{ {src} }}\nlet {n} = c{i}()"
    if kind == "tuple":
        return f"let t{i}: ({ty}, int) = ({src}, 1)\nlet {n} = t{i}.0"
    if kind == "generic":
        return f"let x{i}: {ty} = {src}\nlet {n} = gid(x{i})"
    if kind == "loop":
        return f"let mut {n}: {ty} = 0\nfor _ in 0..2 {{ {n} = {src} }}"
    raise ValueError(kind)


def casts(ty, n, v):
    vs = WRAP[ty](v)
    w32 = (vs % 2**32) - (2**32 if vs % 2**32 >= 2**31 else 0)
    return [
        (f"{n} as int", str(s(vs))),
        (f"{n} as u64", str(u(vs))),
        (f"{n} as i64", str(s(vs))),
        (f"{n} as i32", str(w32)),
        (f"{n} as u8", str(vs % 256)),
        (f"{n}.to_hex()", ("-" + format(-vs, "x")) if vs < 0 else format(vs, "x")),
        (f"{n}.to_string()", str(vs)),
        (f'"{{{n}}}"', str(vs)),
    ]


def generate(rng, index):
    ty = rng.choice(["u64", "i64"])
    env, names, body, out = {}, [], [], []
    trap = False
    for j in range(rng.randint(1, 3)):
        for _ in range(6):
            src, f = expr(rng, ty, names, rng.randint(1, 4))
            try:
                v = f(env)
                trap = False
                break
            except Trap:
                trap = True
        body.append(route(rng.choice(ROUTES), ty, j, src))
        if trap:
            break
        env[f"r{j}"] = v
        names.append(f"r{j}")
    if not trap:
        for n in names:
            for cs, ce in rng.sample(casts(ty, n, env[n]), 3):
                body.append(f"println({cs})")
                out.append(ce)
        if len(names) >= 2:
            a, b = names[0], names[1]
            body.append(f"println({a} < {b})")
            out.append(str(env[a] < env[b]).lower())
            body.append(f"println({a} == {b})")
            out.append(str(env[a] == env[b]).lower())
            body.append(f"let mut sv: Vec<{ty}> = vec![{a}, {b}, {a} - {a}]\nsv.sort()\nprintln(sv[0])\nprintln(sv[2])")
            ss = sorted([env[a], env[b], 0])
            out += [str(ss[0]), str(ss[2])]
    expected = TRAP if trap else "".join(x + "\n" for x in out)
    return HEAD + as_main("\n".join(body), rng.random() < 0.5), expected
