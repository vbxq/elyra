from common import TRAP, I64MIN, I64MAX, tdiv, trem, int_lit as L

DESCRIPTION = "hot loops whose int values cross 2^47 or approach the i64 bounds, stored in vecs, arrays, structs, maps, globals and closures"

P47 = 2**47

KERNELS = {}


def kernel(name):
    def register(f):
        KERNELS[name] = f
        return f
    return register


@kernel('acc_local')
def _(s, st, n):
    code = f'''let mut acc = {L(s)}
let mut i = 0
let mut crossings = 0
while i < {n} {{
    let before = acc < 140737488355328
    acc = acc + {L(st)}
    if before != (acc < 140737488355328) {{ crossings += 1 }}
    i += 1
}}
println(acc)
println(crossings)'''
    acc = s; c = 0
    for _ in range(n):
        b = acc < P47; acc += st
        if not (I64MIN <= acc <= I64MAX): return code, None
        if b != (acc < P47): c += 1
    return code, f'{acc}\n{c}\n'


@kernel('vec_store')
def _(s, st, n):
    code = f'''let mut v: Vec<int> = vec![0; 8]
let mut x = {L(s)}
for i in 0..{n} {{
    x = x + {L(st)}
    v[i % 8] = x
}}
let mut sum = 0
for e in v {{ sum = sum + e / 8 }}
println(v[0])
println(v[7])
println(sum)
v.sort()
println(v[0])
println(v.contains({L(s + st)}))'''
    v = [0] * 8; x = s
    for i in range(n):
        x += st
        if not (I64MIN <= x <= I64MAX): return code, None
        v[i % 8] = x
    sm = 0
    for e in v:
        sm += tdiv(e, 8)
        if not (I64MIN <= sm <= I64MAX): return code, None
    r = f'{v[0]}\n{v[7]}\n{sm}\n'
    v.sort(); r += f'{v[0]}\n{"true" if (s + st) in v else "false"}\n'
    return code, r


@kernel('vec_push')
def _(s, st, n):
    code = f'''let mut v: Vec<int> = vec![]
let mut x = {L(s)}
for i in 0..{n} {{
    x = x + {L(st)}
    v.push(x)
}}
println(v.len())
println(v[{n} - 1])
println(v[{n} / 2])
let w = v.map(fn(q: int) -> int {{ q % 1000 }})
println(w.fold(0, fn(a: int, b: int) -> int {{ a + b }}))
println(v.index_of({L(max(I64MIN, min(I64MAX, s + st * (n // 3 + 1))))}).unwrap_or(-1))'''
    v = []; x = s
    for i in range(n):
        x += st
        if not (I64MIN <= x <= I64MAX): return code, None
        v.append(x)
    fs = sum(trem(q, 1000) for q in v)
    tgt = s + st * (n // 3 + 1)
    idx = v.index(tgt) if tgt in v else -1
    return code, f'{n}\n{v[-1]}\n{v[n // 2]}\n{fs}\n{idx}\n'


@kernel('array_store')
def _(s, st, n):
    code = f'''let mut a = [0, 0, 0, 0]
let mut x = {L(s)}
let mut i = 0
while i < {n} {{
    a[i & 3] = x
    x = x + {L(st)}
    i += 1
}}
println(a[0])
println(a[3])
println(a[1] - a[0])'''
    a = [0] * 4; x = s
    for i in range(n):
        a[i & 3] = x; x += st
        if not (I64MIN <= x <= I64MAX): return code, None
    d = a[1] - a[0]
    if not (I64MIN <= d <= I64MAX): return code, None
    return code, f'{a[0]}\n{a[3]}\n{d}\n'


@kernel('struct_field')
def _(s, st, n):
    code = f'''struct Ctr {{ v: int, hi: int }}
let mut c = Ctr {{ v: {L(s)}, hi: 0 }}
for _ in 0..{n} {{
    c.v = c.v + {L(st)}
    if c.v > c.hi {{ c.hi = c.v }}
}}
println(c.v)
println(c.hi)'''
    v = s; hi = 0
    for _ in range(n):
        v += st
        if not (I64MIN <= v <= I64MAX): return code, None
        if v > hi: hi = v
    return code, f'{v}\n{hi}\n'


@kernel('global')
def _(s, st, n):
    code = f'''let mut G = {L(s)}
let mut H = 0
fn tick(d: int) {{ G = G + d; H += 1 }}
for _ in 0..{n} {{ tick({L(st)}) }}
println(G)
println(H)
println(G > 140737488355327)'''
    g = s
    for _ in range(n):
        g += st
        if not (I64MIN <= g <= I64MAX): return code, None
    return code, f'{g}\n{n}\n{"true" if g > P47 - 1 else "false"}\n'


@kernel('map_count')
def _(s, st, n):
    code = f'''let mut m: Map<int, int> = Map::new()
let mut x = {L(s)}
for i in 0..{n} {{
    x = x + {L(st)}
    let key = x / 1024
    m[key] = match m.get(key) {{ Some(c) => c + 1, None => 1 }}
}}
println(m.len())
let ks = m.keys()
println(ks[0])
println(m[ks[0]])
println(m.contains_key({L(tdiv(s + st, 1024))}))'''
    m = {}; x = s
    for i in range(n):
        x += st
        if not (I64MIN <= x <= I64MAX): return code, None
        key = tdiv(x, 1024); m[key] = m.get(key, 0) + 1
    ks = list(m)
    return code, f'{len(m)}\n{ks[0]}\n{m[ks[0]]}\n{"true" if tdiv(s + st, 1024) in m else "false"}\n'


@kernel('closure_acc')
def _(s, st, n):
    code = f'''let mut total = {L(s)}
let add = fn(d: int) {{ total = total + d }}
for _ in 0..{n} {{ add({L(st)}) }}
println(total)'''
    t = s
    for _ in range(n):
        t += st
        if not (I64MIN <= t <= I64MAX): return code, None
    return code, f'{t}\n'


@kernel('for_range')
def _(s, st, n):
    e = max(I64MIN, min(I64MAX, s + n))
    code = f'''let mut cnt = 0
let mut last = 0
let mut xs = 0
for i in {L(s)}..{L(e)} {{
    cnt += 1
    last = i
    xs = xs ^ i
}}
println(cnt)
println(last)
println(xs)
let mut c2 = 0
for j in {L(s)}..={L(e)} step {max(1, abs(st) % 7 + 1)} {{ c2 += 1 }}
println(c2)'''
    xs = 0
    for i in range(s, e): xs ^= i
    stp = max(1, abs(st) % 7 + 1)
    c2 = len(range(s, e + 1, stp))
    return code, f'{e - s}\n{e - 1 if e > s else 0}\n{xs}\n{c2}\n'


@kernel('match_range')
def _(s, st, n):
    code = f'''fn cls(x: int) -> int {{
    match x {{
        140737488355328 => 1,
        -140737488355329 => 2,
        140737488355320..=140737488355327 => 3,
        140737488355329..=281474976710656 => 4,
        9223372036854775807 => 5,
        -9223372036854775808 => 6,
        _ => if x < 0 {{ 7 }} else {{ 8 }},
    }}
}}
let mut x = {L(s)}
let mut h = 0
for _ in 0..{n} {{
    x = x + {L(st)}
    h = h * 7 + cls(x)
    h = h % 1000000007
}}
println(h)
println(cls(x))'''
    def cls(x):
        if x == P47: return 1
        if x == -P47 - 1: return 2
        if P47 - 8 <= x <= P47 - 1: return 3
        if P47 + 1 <= x <= 2**48: return 4
        if x == I64MAX: return 5
        if x == I64MIN: return 6
        return 7 if x < 0 else 8
    x = s; h = 0
    for _ in range(n):
        x += st
        if not (I64MIN <= x <= I64MAX): return code, None
        h = (h * 7 + cls(x)) % 1000000007
    return code, f'{h}\n{cls(x)}\n'
STARTS = [P47 - 50, -P47 + 50, -P47 - 1, P47, 0, I64MAX - 3000, I64MIN + 3000, 2**48, P47 - 1]
STEPS = [1, -1, 3, -3, 7, 2**40, -2**40, 2**20 + 1]
NS = [10, 100, 300, 2000, 20000]
NAMES = list(KERNELS)


def generate(rng, index):
    name = NAMES[index % len(NAMES)]
    while True:
        s, st, n = rng.choice(STARTS), rng.choice(STEPS), rng.choice(NS)
        code, exp = KERNELS[name](s, st, n)
        if exp is not None:
            break
        if rng.random() < 0.15:
            exp = TRAP
            break
    # kernels that declare a struct or a fn stay at top level
    if rng.random() < 0.5 and name != "global" and "struct " not in code and "fn cls" not in code:
        code = "fn main() {\n" + code + "\n}\nmain()"
    return code + "\n", exp
