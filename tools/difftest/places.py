import copy

from common import wrap_bits

DESCRIPTION = "writes through nested places (vec of structs, map values, 2d arrays, Option slots) with aliasing and clone, in fns, methods, closures and loops"


def w32(x):
    return wrap_bits(x, 32, True)


def w16(x):
    return wrap_bits(x, 16, True)


class P:
    def __init__(self, a):
        self.a, self.b, self.tag, self.xs = a, w32(a), f"p{a}", [a]


class World:
    def __init__(self):
        self.ps = [P(1), P(2), P(3)]
        self.m = {"ka": P(10), "kb": P(20)}
        self.mv = {1: [1, 2], 5: []}
        self.grid = [[0] * 3 for _ in range(3)]
        self.opt = [None, P(9)]
        self.t = (0, "t")
        self.u = {}


def vec(xs):
    return "[" + ", ".join(map(str, xs)) + "]"


def dp(p):
    return f"{p.a}/{p.b}/{p.tag}/{vec(p.xs)}"


def dump(w):
    s = "".join(dp(p) + ";" for p in w.ps) + "|"
    s += "".join(k + "=" + dp(v) + ";" for k, v in w.m.items())
    s += "|{" + ", ".join(f"{k}: {vec(v)}" for k, v in w.mv.items()) + "}|"
    s += "[" + ", ".join(vec(r) for r in w.grid) + "]|"
    s += "".join((dp(o) if o else "-") + ";" for o in w.opt)
    s += f'|({w.t[0]}, "{w.t[1]}")|' + "{" + ", ".join(f"{k}: {v}" for k, v in w.u.items()) + "}"
    return s


PRE = '''struct P { a: int, b: i32, tag: string, xs: Vec<int> }
struct W { ps: Vec<P>, m: Map<string, P>, mv: Map<int, Vec<int>>, grid: [[int; 3]; 3], opt: Vec<Option<P>>, t: (int, string), u: Map<u8, i16> }
fn mk(a: int) -> P { P { a, b: a as i32, tag: "p{a}", xs: vec![a] } }
fn dp(p: P) -> string { "{p.a}/{p.b}/{p.tag}/{p.xs}" }
fn dump(w: W) -> string {
    let mut s = ""
    for p in w.ps { s += dp(p) + ";" }
    s += "|"
    for k in w.m.keys() { s += k + "=" + dp(w.m[k]) + ";" }
    s += "|{w.mv}|{w.grid}|"
    for o in w.opt {
        s += match o {
            Some(p) => dp(p),
            None => "-",
        }
        s += ";"
    }
    s += "|{w.t}|{w.u}"
    s
}
fn init() -> W {
    let mut w = W { ps: vec![mk(1), mk(2), mk(3)], m: Map::new(), mv: Map::new(), grid: [[0; 3]; 3], opt: vec![None, Some(mk(9))], t: (0, "t"), u: Map::new() }
    w.m["ka"] = mk(10)
    w.m["kb"] = mk(20)
    w.mv[1] = vec![1, 2]
    w.mv[5] = vec![]
    w
}
'''

KINDS = ["ps_a", "ps_b", "ps_push", "ps_xs", "m_a", "m_push", "m_ins", "m_share", "mv_push", "mv_new", "mv_idx",
         "grid", "opt_some", "opt_none", "opt_share", "t", "u_set", "u_add", "clone", "alias", "swap", "rot",
         "iflet", "nested"]


def gen_op(w, R, rng, in_loop):
    # returns (source, fn applying the same change to the python model); w is the model state at this point
    kinds = KINDS + ([] if in_loop else ["m_remove"])
    k = rng.choice(kinds)
    n = len(w.ps)
    i, j, v = rng.randrange(n), rng.randrange(n), rng.randint(-40, 40)
    keys = sorted(w.m.keys())
    if k in ("m_a", "m_push") and not keys:
        k = "m_ins"
    if k == "u_add" and not w.u:
        k = "u_set"
    if k == "ps_a":
        return f"{R}.ps[{i}].a += {v}", lambda w: setattr(w.ps[i], "a", w.ps[i].a + v)
    if k == "ps_b":
        K, c = rng.choice([65599, 1103515245, -3, 2147483647]), rng.randint(-9, 9)
        return f"{R}.ps[{i}].b = {R}.ps[{i}].b * {K} + {c}", lambda w: setattr(w.ps[i], "b", w32(w.ps[i].b * K + c))
    if k == "ps_push":
        return f"{R}.ps[{i}].xs.push({v})", lambda w: w.ps[i].xs.append(v)
    if k == "ps_xs":
        jj = 0 if in_loop else rng.randrange(len(w.ps[i].xs))

        def f(w):
            w.ps[i].xs[jj] -= v
        return f"{R}.ps[{i}].xs[{jj}] -= {v}", f
    if k == "m_a":
        key = rng.choice(keys)

        def f(w):
            w.m[key].a += v
        return f'{R}.m["{key}"].a += {v}', f
    if k == "m_push":
        key = rng.choice(keys)
        return f'{R}.m["{key}"].xs.push({v})', lambda w: w.m[key].xs.append(v)
    if k == "m_ins":
        key = rng.choice(["ka", "kb", "kc", "kd"])

        def f(w):
            w.m[key] = P(v)
        return f'{R}.m["{key}"] = mk({v})', f
    if k == "m_share":
        key = rng.choice(["ka", "kc"])

        def f(w):
            w.m[key] = w.ps[i]
        return f'{R}.m["{key}"] = {R}.ps[{i}]', f
    if k == "m_remove":
        key = rng.choice(["ka", "kb", "kc"])
        return f'let _ = {R}.m.remove("{key}")', lambda w: w.m.pop(key, None)
    if k == "mv_push":
        key = rng.choice(sorted(w.mv))
        return f"{R}.mv[{key}].push({v})", lambda w: w.mv[key].append(v)
    if k == "mv_new":
        key = rng.choice([1, 5, 7, -2])

        def f(w):
            w.mv[key] = [v, v + 1]
        return f"{R}.mv[{key}] = vec![{v}, {v + 1}]", f
    if k == "mv_idx":
        cands = sorted(kk for kk in w.mv if w.mv[kk])
        if not cands:
            return f"{R}.mv[1] = vec![{v}]", lambda w: w.mv.__setitem__(1, [v])
        key = rng.choice(cands)
        jj = 0 if in_loop else rng.randrange(len(w.mv[key]))

        def f(w):
            w.mv[key][jj] += v
        return f"{R}.mv[{key}][{jj}] += {v}", f
    if k == "grid":
        a, b = rng.randrange(3), rng.randrange(3)

        def f(w):
            w.grid[a][b] += v
        return f"{R}.grid[{a}][{b}] += {v}", f
    if k == "opt_some":
        oi = rng.randrange(2)

        def f(w):
            w.opt[oi] = P(v)
        return f"{R}.opt[{oi}] = Some(mk({v}))", f
    if k == "opt_none":
        oi = rng.randrange(2)

        def f(w):
            w.opt[oi] = None
        return f"{R}.opt[{oi}] = None", f
    if k == "opt_share":
        oi = rng.randrange(2)

        def f(w):
            w.opt[oi] = w.ps[i]
        return f"{R}.opt[{oi}] = Some({R}.ps[{i}])", f
    if k == "t":
        def f(w):
            w.t = (w.t[0] + v, w.t[1] + "z")
        return f'{R}.t = ({R}.t.0 + {v}, {R}.t.1 + "z")', f
    if k == "u_set":
        kk = rng.randint(0, 255)
        key, val = (200 + kk) & 255, rng.randint(-30000, 30000)

        def f(w):
            w.u[key] = val
        return f"{R}.u[(200 as u8) + {kk}] = {val}", f
    if k == "u_add":
        key, val = rng.choice(sorted(w.u)), rng.randint(20000, 32000)

        def f(w):
            w.u[key] = w16(w.u[key] + val)
        return f"{R}.u[{key}] += {val}", f
    if k == "clone":
        def f(w):
            c = copy.deepcopy(w.ps[i])
            c.a += 1000
            c.xs.append(7)
            w.t = (w.t[0] + c.a + len(c.xs), w.t[1])
        return (f"{{\n    let mut c = {R}.ps[{i}].clone()\n    c.a += 1000\n    c.xs.push(7)\n"
                f"    {R}.t = ({R}.t.0 + c.a + c.xs.len(), {R}.t.1)\n}}"), f
    if k == "alias":
        def f(w):
            r = w.ps[i]
            r.a += v
            r.xs.append(v)
        return f"{{\n    let mut r = {R}.ps[{i}]\n    r.a += {v}\n    r.xs.push({v})\n}}", f
    if k == "swap":
        def f(w):
            w.ps[i], w.ps[j] = w.ps[j], w.ps[i]
        return f"{R}.ps.swap({i}, {j})", f
    if k == "rot":
        def f(w):
            w.ps.append(w.ps.pop(i))
        return f"{{\n    let x = {R}.ps.remove({i})\n    {R}.ps.push(x)\n}}", f
    if k == "iflet":
        oi = rng.randrange(2)

        def f(w):
            if w.opt[oi] is not None:
                w.ps[j].a += w.opt[oi].a
            else:
                w.ps[j].a -= 1
        return f"if let Some(p) = {R}.opt[{oi}] {{ {R}.ps[{j}].a += p.a }} else {{ {R}.ps[{j}].a -= 1 }}", f

    def f(w):
        w.ps[len(w.ps[i].xs) % len(w.ps)].a += 1
    return f"{R}.ps[{R}.ps[{i}].xs.len() % {R}.ps.len()].a += 1", f


def indent(text, n):
    pad = "    " * n
    return "\n".join(pad + line if line else line for line in text.split("\n"))


CONTEXTS = ["top", "fn", "method", "closure_top", "closure_local", "generic", "loop", "loop_method"]


def generate(rng, index):
    ctx = CONTEXTS[index % len(CONTEXTS)]
    R = "self" if ctx in ("method", "loop_method") else "w"
    in_loop = ctx.startswith("loop")
    sim = World()
    codes, fns = [], []
    for _ in range(rng.randint(6, 18)):
        c, f = gen_op(sim, R, rng, in_loop)
        f(sim)
        codes.append(c)
        fns.append(f)
    sim = World()
    for _ in range(2 if in_loop else 1):
        for f in fns:
            f(sim)
    body = "\n".join(codes)
    if ctx == "top":
        src = "let mut w = init()\n" + body + "\nprintln(dump(w))\n"
    elif ctx == "fn":
        src = "fn run(mut w: W) {\n" + indent(body, 1) + "\n}\nlet mut w = init()\nrun(w)\nprintln(dump(w))\n"
    elif ctx == "method":
        src = ("impl W {\n    fn run(mut self) {\n" + indent(body, 2)
               + "\n    }\n}\nlet mut w = init()\nw.run()\nprintln(dump(w))\n")
    elif ctx == "closure_top":
        src = "let mut w = init()\nlet f = fn() {\n" + indent(body, 1) + "\n}\nf()\nprintln(dump(w))\n"
    elif ctx == "closure_local":
        src = ("fn host() -> string {\n    let mut w = init()\n    let f = fn() {\n" + indent(body, 2)
               + "\n    }\n    f()\n    dump(w)\n}\nprintln(host())\n")
    elif ctx == "generic":
        src = ("fn run<T>(mut w: W, x: T) -> T {\n" + indent(body, 1)
               + "\n    x\n}\nlet mut w = init()\nlet _ = run(w, \"s\")\nprintln(dump(w))\n")
    elif ctx == "loop":
        src = ("let mut w = init()\n'outer: for it in 0..2 {\n    for jj in 0..3 {\n        if jj == 1 { continue 'outer }\n"
               + indent(body, 2) + "\n    }\n}\nprintln(dump(w))\n")
    else:
        src = ("impl W {\n    fn run(mut self) {\n        let mut n = 0\n        loop {\n            n += 1\n"
               "            if n > 2 { break }\n" + indent(body, 3) + "\n        }\n    }\n}\nlet mut w = init()\nw.run()\nprintln(dump(w))\n")
    return PRE + src, dump(sim) + "\n"
