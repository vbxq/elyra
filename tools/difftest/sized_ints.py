import operator

from common import tdiv, trem, wrap_bits

DESCRIPTION = "i8..u32 wrapping arithmetic through closures, generics, traits, map keys, compound assignment and range patterns"

TYPES = {"u8": (8, False), "i8": (8, True), "u16": (16, False), "i16": (16, True), "u32": (32, False), "i32": (32, True)}
BIN = {"+": operator.add, "-": operator.sub, "*": operator.mul, "&": operator.and_, "|": operator.or_, "^": operator.xor}


def wrap(t, x):
    bits, sg = TYPES[t]
    return wrap_bits(x, bits, sg)


def rand_val(rng, t):
    bits, sg = TYPES[t]
    if sg:
        return rng.randint(-(1 << (bits - 1)), (1 << (bits - 1)) - 1)
    return rng.randint(0, (1 << bits) - 1)


def lit(rng, t):
    bits, sg = TYPES[t]
    v = rng.choice([0, 1, 2, 3, 7, 100, (1 << (bits - 1)) - 1, rand_val(rng, t)])
    if sg and rng.random() < 0.3:
        v = -abs(v) if v != 0 else -1
    return wrap(t, v) if (sg or v >= 0) else abs(v)


def expr(rng, t, depth):
    bits, sg = TYPES[t]
    if depth == 0 or rng.random() < 0.25:
        c = rng.random()
        if c < 0.4:
            return "a", lambda e: e["a"]
        if c < 0.7:
            return "b", lambda e: e["b"]
        v = lit(rng, t)
        return (f"({v})" if v < 0 else str(v)), (lambda e, v=v: v)
    op = rng.choice(["+", "-", "*", "&", "|", "^", "<<", ">>", "/", "%", "neg", "not", "+", "*", "-"])
    if op in ("neg", "not"):
        s, f = expr(rng, t, depth - 1)
        if op == "neg":
            return f"-({s})", lambda e: wrap(t, -f(e))
        return f"~({s})", lambda e: wrap(t, ~f(e))
    if op in ("<<", ">>"):
        s, f = expr(rng, t, depth - 1)
        n = rng.choice([0, 1, 3, 7, 8, 15, 16, 31, 33, rng.randint(0, 40)])
        if op == "<<":
            return f"({s} << {n})", lambda e: wrap(t, f(e) << (n % bits))
        return f"({s} >> {n})", lambda e: wrap(t, f(e) >> (n % bits)) if sg else f(e) >> (n % bits)
    if op in ("/", "%"):
        s, f = expr(rng, t, depth - 1)
        d = rng.choice([1, 2, 3, 7, 10, 127])
        if sg and rng.random() < 0.3:
            d = -d
        ds = f"({d})" if d < 0 else str(d)
        if op == "/":
            return f"({s} / {ds})", lambda e: wrap(t, tdiv(f(e), d))
        return f"({s} % {ds})", lambda e: wrap(t, trem(f(e), d))
    s1, f1 = expr(rng, t, depth - 1)
    s2, f2 = expr(rng, t, depth - 1)
    o = BIN[op]
    return f"({s1} {op} {s2})", lambda e: wrap(t, o(f1(e), f2(e)))


ROUTES = ["direct", "closure", "generic_ap", "box", "trait", "mapkey", "vec_compound", "tuple", "ret_tail", "match_arm"]


def L(v):
    return f"({v})" if v < 0 else str(v)


def generate(rng, index):
    t = rng.choice(list(TYPES))
    kind = ROUTES[index % len(ROUTES)]
    A, B = rand_val(rng, t), rand_val(rng, t)
    src, f = expr(rng, t, rng.randint(1, 4))
    while kind == "ret_tail" and "a" not in src and "b" not in src:
        src, f = expr(rng, t, rng.randint(1, 4))
    val = f({"a": A, "b": B})
    head = f"let a0: {t} = {L(A)}\nlet b0: {t} = {L(B)}\n"
    if kind == "direct":
        code = head + f"let a = a0\nlet b = b0\nlet r: {t} = {src}\nprintln(r)\nprintln(r as int)\n"
        exp = f"{val}\n{val}\n"
    elif kind == "closure":
        code = head + f"let f = fn(a: {t}, b: {t}) -> {t} {{ {src} }}\nprintln(f(a0, b0))\nprintln(f(b0, a0) == f(b0, a0))\n"
        exp = f"{val}\ntrue\n"
    elif kind == "generic_ap":
        code = ("fn ap<X>(f: fn(X, X) -> X, a: X, b: X) -> X { f(a, b) }\nfn id<X>(x: X) -> X { x }\n" + head
                + f"println(id(ap(fn(a: {t}, b: {t}) -> {t} {{ {src} }}, a0, b0)))\n")
        exp = f"{val}\n"
    elif kind == "box":
        code = ("struct Bx<X> { v: X }\nfn unbox<X>(b: Bx<X>) -> X { b.v }\n" + head
                + f"let ba: Bx<{t}> = Bx {{ v: a0 }}\nlet bb = Bx {{ v: b0 }}\nlet a = unbox(ba)\nlet b = bb.v\n"
                f"let r: {t} = {src}\nlet br = Bx {{ v: r }}\nprintln(br.v)\n")
        exp = f"{val}\n"
    elif kind == "trait":
        code = (f"trait Op {{ fn run(self, b: {t}) -> {t}; fn twice(self, b: {t}) -> {t} {{ self.run(b) }} }}\n"
                f"struct Wr {{ v: {t} }}\nimpl Op for Wr {{\n    fn run(self, b: {t}) -> {t} {{\n        let a = self.v\n"
                f"        {src}\n    }}\n}}\nfn go<T: Op>(x: T, b: {t}) -> {t} {{ x.twice(b) }}\n" + head
                + "println(go(Wr { v: a0 }, b0))\n")
        exp = f"{val}\n"
    elif kind == "mapkey":
        code = head + (f"let a = a0\nlet b = b0\nlet mut m: Map<{t}, int> = Map::new()\nm[{src}] = 1\nm[a] = 2\n"
                       f"m[{src}] += 10\nprintln(m)\nprintln(m.contains_key({src}))\n")
        d = {val: 1}
        d[A] = 2
        d[val] += 10
        exp = "{" + ", ".join(f"{k}: {v}" for k, v in d.items()) + "}\ntrue\n"
    elif kind == "vec_compound":
        code = head + f"let a = a0\nlet b = b0\nlet mut v: Vec<{t}> = vec![a, b]\nv[0] += {src}\nv[1] *= 3\nv.push({src})\nprintln(v)\n"
        exp = f"[{wrap(t, A + val)}, {wrap(t, B * 3)}, {val}]\n"
    elif kind == "tuple":
        code = head + (f"fn pack(a: {t}, b: {t}) -> ({t}, {t}, int) {{ ({src}, a, (a as int) - (b as int)) }}\n"
                       "let p = pack(a0, b0)\nprintln(p)\nlet (x, y, z) = p\nprintln(x)\n")
        exp = f"({val}, {A}, {A - B})\n{val}\n"
    elif kind == "ret_tail":
        code = head + (f"fn calc(a: {t}, b: {t}) -> {t} {{\n    if a == b {{ {src} }} else {{\n        let r = {src}\n"
                       "        r\n    }\n}\nprintln(calc(a0, b0))\n")
        exp = f"{val}\n"
    else:
        bits, sg = TYPES[t]
        lo = -(1 << (bits - 1)) if sg else 0
        hi = (1 << (bits - 1)) - 1 if sg else (1 << bits) - 1
        # a range reaching the maximum would make the wildcard arm unreachable
        mid = rng.randint(lo, hi - 1)
        code = head + (f"let a = a0\nlet b = b0\nlet r: {t} = {src}\nlet k = match r {{\n    {lo}..={mid} => 1,\n"
                       "    _ => 2,\n}\nprintln(k)\n")
        exp = f"{1 if val <= mid else 2}\n"
    return code, exp
