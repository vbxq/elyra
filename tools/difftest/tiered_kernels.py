import functools

from common import TRAP, Trap, chk, tdiv, trem

DESCRIPTION = "a random checked-int function called thousands of times from loops, arrays, closures, recursion and vecs so it tiers up"

M = 1000003


def expr(rng, d, names):
    if d == 0 or rng.random() < 0.3:
        if rng.random() < 0.6:
            v = rng.choice(names)
            return v, (lambda e, v=v: e[v])
        c = rng.randint(-50, 50) if rng.random() < 0.9 else rng.choice([1 << 20, 99991, -(1 << 30), 3037000500, 1 << 40])
        return (f"({c})" if c < 0 else str(c)), (lambda e, c=c: c)
    op = rng.choice(["+", "-", "*", "/", "%", "&", "|", "^", ">>", "<<", "if", "min", "call"])
    s1, f1 = expr(rng, d - 1, names)
    s2, f2 = expr(rng, d - 1, names)
    if op == "+":
        return f"({s1} + {s2})", lambda e: chk(f1(e) + f2(e))
    if op == "-":
        return f"({s1} - {s2})", lambda e: chk(f1(e) - f2(e))
    if op == "*":
        return f"({s1} * {s2})", lambda e: chk(f1(e) * f2(e))
    if op == "/":
        return f"({s1} / ({s2} | 1))", lambda e: chk(tdiv(f1(e), f2(e) | 1))
    if op == "%":
        return f"({s1} % ({s2} | 1))", lambda e: trem(f1(e), f2(e) | 1)
    if op == "&":
        return f"({s1} & {s2})", lambda e: f1(e) & f2(e)
    if op == "|":
        return f"({s1} | {s2})", lambda e: f1(e) | f2(e)
    if op == "^":
        return f"({s1} ^ {s2})", lambda e: f1(e) ^ f2(e)
    if op == ">>":
        n = rng.randint(0, 9)
        return f"({s1} >> {n})", lambda e: f1(e) >> n
    if op == "<<":
        n = rng.randint(0, 6)
        return f"({s1} << {n})", lambda e: chk(f1(e) << n)
    if op == "if":
        s3, f3 = expr(rng, d - 1, names)
        return f"(if {s1} < {s2} {{ {s3} }} else {{ {s2} }})", lambda e: f3(e) if f1(e) < f2(e) else f2(e)
    if op == "min":
        return f"mn({s1}, {s2})", lambda e: min(f1(e), f2(e))
    return f"h({s1}, {s2})", lambda e: hpy(f1(e), f2(e))


def hpy(a, b):
    return chk(chk(a * 3) - b) if a > b else chk(b - a)


SHAPES = ["plain", "loop", "array", "closure", "recurse", "vec"]
HEAD = ("fn mn(x: int, y: int) -> int { if x < y { x } else { y } }\n"
        "fn h(x: int, y: int) -> int { if x > y { x * 3 - y } else { y - x } }\n")


def generate(rng, index):
    shape = SHAPES[index % len(SHAPES)]
    s, f0 = expr(rng, rng.randint(2, 5), ["a", "b"])

    def f(a, b):
        return f0({"a": a, "b": b})

    iters = rng.choice([3000, 12000, 25000])
    src = HEAD + f"fn f(a: int, b: int) -> int {{ {s} }}\n"
    try:
        if shape == "plain":
            src += (f"let mut acc = 0\nlet mut i = 0\nwhile i < {iters} {{\n    acc = (acc + f(i % 97 - 40, i % 13 - 6)) % {M}\n"
                    "    i += 1\n}\nprintln(acc)\n")
            acc = 0
            for i in range(iters):
                acc = trem(chk(acc + f(i % 97 - 40, i % 13 - 6)), M)
            printed = [acc]
        elif shape == "loop":
            src += (f"fn run(n: int) -> int {{\n    let mut acc = 0\n    for i in 0..n {{\n        let x = f(i % 31 - 15, (i * 7) % 19 - 9)\n"
                    f"        if x % 2 == 0 {{ acc += x % 1000 }} else {{ acc -= x % 777 }}\n        acc = acc % {M}\n    }}\n"
                    f"    acc\n}}\nprintln(run({iters}))\nprintln(run(17))\n")

            def run(n):
                acc = 0
                for i in range(n):
                    x = f(i % 31 - 15, (i * 7) % 19 - 9)
                    if trem(x, 2) == 0:
                        acc = chk(acc + trem(x, 1000))
                    else:
                        acc = chk(acc - trem(x, 777))
                    acc = trem(acc, M)
                return acc
            printed = [run(iters), run(17)]
        elif shape == "array":
            src += (f"fn run(n: int) -> int {{\n    let mut arr = [0; 64]\n    for i in 0..n {{\n        let k = i % 64\n"
                    f"        arr[k] = (arr[k] + f(k - 32, i % 11)) % {M}\n    }}\n    let mut t = 0\n"
                    f"    for x in arr {{ t = (t * 31 + x) % {M} }}\n    t\n}}\nprintln(run({iters}))\n")
            arr = [0] * 64
            for i in range(iters):
                k = i % 64
                arr[k] = trem(chk(arr[k] + f(k - 32, i % 11)), M)
            t = 0
            for x in arr:
                t = trem(chk(t * 31 + x), M)
            printed = [t]
        elif shape == "closure":
            src += (f"fn run(n: int, g: fn(int, int) -> int) -> int {{\n    let mut acc = 0\n"
                    f"    for i in 0..n {{ acc = (acc + g(i % 50 - 25, i % 9)) % {M} }}\n    acc\n}}\nlet k = 3\n"
                    f"println(run({iters}, fn(x: int, y: int) -> int {{ f(x, y) + k }}))\n")
            acc = 0
            for i in range(iters):
                acc = trem(chk(acc + chk(f(i % 50 - 25, i % 9) + 3)), M)
            printed = [acc]
        elif shape == "recurse":
            src += (f"fn rec(n: int, a: int) -> int {{\n    if n <= 0 {{ return f(a, n) % 1000 }}\n"
                    f"    (rec(n - 1, a + 1) + rec(n - 2, a - 1)) % {M}\n}}\nlet mut t = 0\n"
                    f"for a in 0..8 {{ t = (t + rec(14, a - 4)) % {M} }}\nprintln(t)\n")

            @functools.lru_cache(None)
            def rec(n, a):
                if n <= 0:
                    return trem(f(a, n), 1000)
                return trem(chk(rec(n - 1, a + 1) + rec(n - 2, a - 1)), M)
            t = 0
            for a in range(8):
                t = trem(chk(t + rec(14, a - 4)), M)
            printed = [t]
        else:
            src += (f"fn run(n: int) -> int {{\n    let mut v: Vec<int> = vec![]\n    for i in 0..n {{\n        v.push(f(i % 40 - 20, i % 7))\n"
                    f"        if v.len() > 50 {{ let _ = v.remove(0) }}\n    }}\n"
                    f"    v.fold(0, fn(s: int, x: int) -> int {{ (s + x) % {M} }})\n}}\nprintln(run({iters}))\n")
            v = []
            for i in range(iters):
                v.append(f(i % 40 - 20, i % 7))
                if len(v) > 50:
                    v.pop(0)
            t = 0
            for x in v:
                t = trem(chk(t + x), M)
            printed = [t]
    except Trap:
        return src, TRAP
    return src, "".join(f"{x}\n" for x in printed)
