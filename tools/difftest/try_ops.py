DESCRIPTION = "the ? operator on Result and Option through From conversion, closures, generics, methods, loops and while let"

PRE = '''enum E1 { Bad(int), Neg }
struct E2 { code: int }
impl From<E1> for E2 {
    fn from(e: E1) -> E2 {
        match e {
            E1::Bad(n) => E2 { code: n },
            E1::Neg => E2 { code: -1 },
        }
    }
}
fn step_a(x: int) -> Result<int, E1> {
    if x > @LIM { Err(E1::Bad(x)) } else if x < 0 { Err(E1::Neg) } else { Ok(x * 2 + 1) }
}
fn step_o(x: int) -> Option<int> {
    if x % @KO == 0 { None } else { Some(x + 3) }
}
fn twice<T>(f: fn(int) -> Result<int, E1>, x: int, tag: T) -> Result<int, E1> {
    let a = f(x)?
    f(a)
}
struct Calc { lim: int }
impl Calc {
    fn chk(self, x: int) -> Result<int, E2> {
        if x > self.lim { return Err(E2 { code: 900 }) }
        Ok(x)
    }
    fn run(self, x: int) -> Result<int, E2> {
        let a = self.chk(x)?
        let b = step_a(a % 50)?
        Ok(a + b)
    }
}
'''


def cmod(a, b):
    r = abs(a) % abs(b)
    return -r if a < 0 else r


class Er(Exception):
    def __init__(self, code):
        self.code = code


def stage(rng, name, step_a, step_o):
    f = rng.choice("ABCDEFGHI")
    C, L = rng.randint(-20, 20), rng.randint(10, 300)
    if f == "A":
        src = f"fn {name}(x: int) -> Result<int, E2> {{\n    let v = step_a(x + {C})?\n    Ok(v - {abs(C)})\n}}\n"
        return f, src, lambda x: step_a(x + C) - abs(C)
    if f == "B":
        src = (f"fn {name}(x: int) -> Result<int, E2> {{\n    let g = fn(y: int) -> Option<int> {{\n        let z = step_o(y)?\n"
               "        Some(z * 2)\n    }\n    match g(x) {\n        Some(v) => Ok(v),\n        None => Err(E2 { code: 500 + x }),\n    }\n}\n")

        def py(x):
            z = step_o(x)
            if z is None:
                raise Er(500 + x)
            return z * 2
        return f, src, py
    if f == "C":
        src = f"fn {name}(x: int) -> Result<int, E2> {{ Calc {{ lim: {L} }}.run(x) }}\n"

        def py(x):
            if x > L:
                raise Er(900)
            return x + step_a(cmod(x, 50))
        return f, src, py
    if f == "D":
        src = (f"fn {name}(x: int) -> Result<int, E2> {{\n    let r = twice(fn(y: int) -> Result<int, E1> {{ if y > {L} "
               f"{{ Err(E1::Bad(y)) }} else {{ Ok(y + 7) }} }}, x, 3)?\n    Ok(r)\n}}\n")

        def py(x):
            for _ in range(2):
                if x > L:
                    raise Er(x)
                x = x + 7
            return x
        return f, src, py
    if f == "E":
        src = (f"fn {name}(x: int) -> Result<int, E2> {{\n    let mut acc = x\n    for i in 0..3 {{\n"
               "        acc = step_a(acc % 40 + i)?\n    }\n    Ok(acc)\n}\n")

        def py(x):
            acc = x
            for i in range(3):
                acc = step_a(cmod(acc, 40) + i)
            return acc
        return f, src, py
    if f == "F":
        src = f"fn {name}(x: int) -> Result<int, E2> {{\n    if step_a(x % 60)? > 30 {{ Ok(x - 1) }} else {{ Ok(x + 1) }}\n}}\n"
        return f, src, lambda x: x - 1 if step_a(cmod(x, 60)) > 30 else x + 1
    if f == "G":
        src = (f"fn {name}(x: int) -> Result<int, E2> {{\n    let v = vec![3, 1, 4, 1, 5, 9, 2]\n"
               "    let pick = fn(i: int) -> Option<int> {\n        let a = v.get(i)?\n        let b = v.get(a)?\n"
               "        Some(a * 10 + b)\n    }\n    match pick((x % 9 + 9) % 9) {\n        Some(r) => Ok(x + r),\n"
               "        None => Err(E2 { code: 77 }),\n    }\n}\n")

        def py(x):
            v = [3, 1, 4, 1, 5, 9, 2]
            i = cmod(cmod(x, 9) + 9, 9)
            if i >= len(v):
                raise Er(77)
            a = v[i]
            if a >= len(v):
                raise Er(77)
            return x + a * 10 + v[a]
        return f, src, py
    if f == "H":
        src = (f"fn {name}(x: int) -> Result<int, E2> {{\n    let h = fn(y: int) -> Result<int, E2> {{\n"
               f"        let a = step_a(y)?\n        Ok(a + {C})\n    }}\n    let r = h(x % 100)?\n    Ok(r)\n}}\n")
        return f, src, lambda x: step_a(cmod(x, 100)) + C
    src = (f"fn {name}(x: int) -> Result<int, E2> {{\n    let mut out = 0\n    let mut k = x\n"
           "    while let Some(n) = step_o(k) {\n        out += n\n        k = n % 23\n"
           f"        if out > {L} {{ return Err(E2 {{ code: out }}) }}\n    }}\n    Ok(out + k)\n}}\n")

    def py(x):
        out, k = 0, x
        while True:
            n = step_o(k)
            if n is None:
                break
            out += n
            k = cmod(n, 23)
            if out > L:
                raise Er(out)
        return out + k
    return f, src, py


def generate(rng, index):
    LIM, KO = rng.randint(60, 400), rng.randint(2, 9)

    def step_a(x):
        if x > LIM:
            raise Er(x)
        if x < 0:
            raise Er(-1)
        return x * 2 + 1

    def step_o(x):
        return None if cmod(x, KO) == 0 else x + 3

    src = PRE.replace("@LIM", str(LIM)).replace("@KO", str(KO))
    stages = []
    ns = rng.randint(2, 5)
    for si in range(ns):
        _, s, py = stage(rng, f"s{si}", step_a, step_o)
        src += s
        stages.append(py)
    src += "fn pipe(x: int) -> Result<int, E2> {\n"
    for si in range(ns):
        src += f"    let v{si} = s{si}({'x' if si == 0 else f'v{si - 1}'})?\n"
    src += f"    Ok(v{ns - 1})\n}}\n"
    ins = sorted(set(rng.randint(-10, 200) for _ in range(10)))
    src += (f"for x in vec![{', '.join(map(str, ins))}] {{\n    match pipe(x) {{\n"
            "        Ok(v) => println(\"{x} ok {v}\"),\n        Err(e) => println(\"{x} err {e.code}\"),\n    }\n}\n")
    out = []
    for x in ins:
        try:
            v = x
            for py in stages:
                v = py(v)
            out.append(f"{x} ok {v}\n")
        except Er as e:
            out.append(f"{x} err {e.code}\n")
    return src, "".join(out)
