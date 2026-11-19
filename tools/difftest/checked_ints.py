from common import TRAP, Trap, chk, tdiv, trem, int_lit, as_main, I64MIN, I64MAX

DESCRIPTION = "checked 64-bit int expressions near the 2^47 and i64 edges, routed through containers, closures and globals"

VALS = [0, 1, -1, 2, 7, 2**47 - 1, 2**47, -2**47, -2**47 - 1, 2**47 + 1, 2**48, -2**48, 2**31, 2**32 + 3,
        2**62, -2**62, I64MAX, I64MIN, 0x0123456789ABCDEF, -0x0FEDCBA987654321, 3037000499, 3037000500, 2**40 + 17]

CMP = {"<": lambda x, y: x < y, "<=": lambda x, y: x <= y, ">": lambda x, y: x > y,
       ">=": lambda x, y: x >= y, "==": lambda x, y: x == y, "!=": lambda x, y: x != y}
BIN = {"+": lambda x, y: chk(x + y), "-": lambda x, y: chk(x - y), "*": lambda x, y: chk(x * y),
       "/": lambda x, y: chk(tdiv(x, y)), "%": trem, "&": lambda x, y: x & y, "|": lambda x, y: x | y,
       "^": lambda x, y: x ^ y}


def expr(rng, names, depth):
    if depth == 0 or rng.random() < 0.3:
        if names and rng.random() < 0.6:
            n = rng.choice(names)
            return n, (lambda env, n=n: env[n])
        v = rng.choice(VALS + [rng.randint(-1000, 1000)])
        return int_lit(v), (lambda env, v=v: v)
    op = rng.choice(["+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>", "neg", "not", "+", "-", "*", "cmp"])
    if op in ("neg", "not"):
        s, f = expr(rng, names, depth - 1)
        if op == "neg":
            return f"(-{s})", (lambda env: chk(-f(env)))
        return f"(~{s})", (lambda env: ~f(env))
    a, fa = expr(rng, names, depth - 1)
    if op in ("<<", ">>"):
        k = rng.choice([0, 1, 3, 15, 16, 17, 31, 32, 47, 48, 62, 63])
        if op == "<<":
            return f"({a} << {k})", (lambda env: chk(fa(env) << k))
        return f"({a} >> {k})", (lambda env: fa(env) >> k)
    b, fb = expr(rng, names, depth - 1)
    if op == "cmp":
        c = rng.choice(list(CMP))
        return f"(if {a} {c} {b} {{ 1 }} else {{ 0 }})", (lambda env: 1 if CMP[c](fa(env), fb(env)) else 0)
    fn = BIN[op]
    return f"({a} {op} {b})", (lambda env: fn(fa(env), fb(env)))


ROUTES = ["plain", "struct", "enum", "tuple", "vec_push", "vec_set", "vec_lit", "array", "map_val", "map_key",
          "closure", "upval", "global", "fnarg", "option", "generic", "loop_acc", "vec_fill", "vec_insert",
          "vec_extend", "vec_map"]


def route(kind, i, src):
    r = f"r{i}"
    if kind == "plain":
        return "", f"let {r} = {src}"
    if kind == "struct":
        return "", f"let mut s{i} = Box {{ v: 0 }}\ns{i}.v = {src}\nlet {r} = s{i}.v"
    if kind == "enum":
        return "", f"let e{i} = Wr::A({src})\nlet {r} = match e{i} {{ Wr::A(q) => q, Wr::B => 0 }}"
    if kind == "tuple":
        return "", f"let t{i} = (1, {src}, 2)\nlet {r} = t{i}.1"
    if kind == "vec_push":
        return "", f"let mut v{i}: Vec<int> = vec![1, 2]\nv{i}.push({src})\nlet {r} = v{i}[2]"
    if kind == "vec_set":
        return "", f"let mut v{i} = vec![5, 6, 7]\nv{i}[1] = {src}\nlet {r} = v{i}[1] + 0 * v{i}[2]"
    if kind == "vec_lit":
        return "", f"let x{i} = {src}\nlet v{i} = vec![3, x{i}, 4]\nlet {r} = v{i}[1]"
    if kind == "array":
        return "", f"let mut a{i} = [0; 3]\na{i}[2] = {src}\nlet {r} = a{i}[2]"
    if kind == "map_val":
        return "", f"let mut m{i}: Map<int, int> = Map::new()\nm{i}[7] = {src}\nlet {r} = m{i}[7]"
    if kind == "map_key":
        return "", (f"let mut m{i}: Map<int, int> = Map::new()\nlet k{i} = {src}\nm{i}[k{i}] = 1\nm{i}[3] = 2\n"
                    f"let {r} = if m{i}.contains_key(k{i}) {{ m{i}.keys()[0] }} else {{ 0 - 424242 }}")
    if kind == "closure":
        return "", f"let c{i} = fn() -> int {{ {src} }}\nlet {r} = c{i}()"
    if kind == "upval":
        return "", f"let mut u{i} = 0\nlet set{i} = fn() {{ u{i} = {src} }}\nset{i}()\nlet {r} = u{i}"
    if kind == "global":
        return f"let mut G{i} = 0\nfn gset{i}(x: int) {{ G{i} = x }}\n", f"gset{i}({src})\nlet {r} = G{i}"
    if kind == "fnarg":
        return f"fn ident{i}(x: int) -> int {{ x }}\n", f"let {r} = ident{i}({src})"
    if kind == "option":
        return "", f"let o{i}: Option<int> = Some({src})\nlet {r} = o{i}.unwrap_or(0)"
    if kind == "generic":
        return "", f"let {r} = gid({src})"
    if kind == "loop_acc":
        return "", f"let mut {r} = 0\nfor _ in 0..3 {{ {r} = {src} }}"
    if kind == "vec_fill":
        return "", f"let mut v{i} = vec![0, 0, 0]\nv{i}.fill({src})\nlet {r} = v{i}[2]"
    if kind == "vec_insert":
        return "", f"let mut v{i} = vec![0, 0]\nv{i}.insert(1, {src})\nlet {r} = v{i}[1]"
    if kind == "vec_extend":
        return "", f"let mut v{i} = vec![0, 0]\nlet w{i} = vec![{src}]\nv{i}.extend(w{i})\nlet {r} = v{i}[2]"
    if kind == "vec_map":
        return "", f"let v{i} = vec![1, 2].map(fn(q: int) -> int {{ q - q + {src} }})\nlet {r} = v{i}[1]"
    raise ValueError(kind)


HEAD = "struct Box { v: int }\nenum Wr { A(int), B }\nfn gid<T>(x: T) -> T { x }\n"


def generate(rng, index):
    env, pre, body, names = {}, [], [], []
    trap = False
    for j in range(rng.randint(1, 3)):
        for _ in range(1 if rng.random() < 0.1 else 6):
            s, f = expr(rng, names, rng.randint(1, 3))
            try:
                val = f(env)
                trap = False
                break
            except Trap:
                trap = True
        p, st = route(rng.choice(ROUTES), j, s)
        pre.append(p)
        body.append(st)
        if trap:
            break
        env[f"r{j}"] = val
        names.append(f"r{j}")
    if trap:
        expected = TRAP
    else:
        for n in names:
            body.append(f"println({n})")
            body.append(f'println("{{{n}}}")')
        expected = "".join(f"{env[n]}\n{env[n]}\n" for n in names)
    return HEAD + "".join(pre) + as_main("\n".join(body), rng.random() < 0.5), expected
