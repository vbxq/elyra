from common import int_lit

DESCRIPTION = "frame-JIT stress: raw int registers at the 2^47 or i64 edges, quick-tier exits, storage caches against vec reallocation and aliasing, enums under allocation, global writes and late global reads"

M = "0xFFFFFFF"


class Gen:
    def __init__(self, rng):
        self.r = rng
        self.edge = rng.choice([47, 63])
        self.IMAX = (1 << self.edge) - 1
        self.IMIN = -(1 << self.edge)
        self.trap = self.r.random() < 0.3
        self.n_outer = self.r.choice([60, 200, 400, 900])
        self.n_inner = self.r.choice([20, 60, 150, 400])
        self.long = self.edge == 47 and self.r.random() < 0.3
        self.trap_at = self.r.randint(1, self.n_outer * self.n_inner // 2 + 1)
        self.uid = 0

    def ch(self, *xs):
        return self.r.choice(xs)

    def fresh(self, p):
        self.uid += 1
        return f"{p}{self.uid}"

    def iv(self, env):
        return self.r.choice(env)

    def atom(self, env):
        k = self.r.random()
        if k < 0.4:
            return self.iv(env)
        if k < 0.5:
            return str(self.ch(0, 1, 3, 7, 255, 4096, 65535, -1, -9, 1 << 20))
        if k < 0.58:
            return f"v[({self.iv(env)} & 0xFFFF) % v.len()]"
        if k < 0.63:
            return f"ar[{self.iv(env)} & 7]"
        if k < 0.68:
            return f"s.{self.ch('a', 'd')}"
        if k < 0.72:
            return self.ch("g0", "g1", "gread()")
        if k < 0.76:
            return "v.len()"
        if k < 0.8:
            return f"eval_e(mk_e({self.iv(env)}))"
        if k < 0.84:
            return f"({self.iv(env)} & 0xFFFFF).to_string().len()"
        return self.iv(env)

    def expr(self, env, d=0):
        if d > 2 or self.r.random() < 0.3:
            return self.atom(env)
        a = self.expr(env, d + 1)
        b = self.expr(env, d + 1)
        o = self.r.random()
        if o < 0.15:
            return f"(({a} + {b}) & {M})"
        if o < 0.25:
            return f"({a} - {b})"
        if o < 0.35:
            return f"(({a} & 0xFFFFF) * ({b} & 0x3FFFFF))"
        if o < 0.42:
            return f"({a} / ((({b}) & 7) + 1))"
        if o < 0.47:
            return f"({a} % ((({b}) & 15) + 1))"
        if o < 0.55:
            return f"({a} {self.ch('&', '|', '^')} {b})"
        if o < 0.62:
            return f"(({a} & 0xFFFFFF) << ({b} & 15))"
        if o < 0.66:
            return f"({a} >> ({b} & 31))"
        if o < 0.7:
            return f"(-{a})"
        if o < 0.78:
            sh = self.ch("8", "16", "24", "0", "47", f"({b} & 63)")
            return f"(({a} & 0x7FFFFF).mul_shr(({b} & 0x7FFFFF) | 1, {sh}))"
        if o < 0.84:
            sh = self.ch("4", "16", "8", f"({b} & 15)")
            return f"(({a} & 0xFFFFFF).shl_div(({b} & 0xFFFF) + 1, {sh}))"
        if o < 0.9:
            return f"{self.ch('fa', 'fb', 'gfun')}({a} & 0xFFFF)"
        return f"(({a} ^ {b}) & {M})"

    def cond(self, env):
        a = self.expr(env, 2)
        b = self.expr(env, 2)
        k = self.r.random()
        if k < 0.4:
            return f"{a} {self.ch('<', '<=', '>', '>=', '==', '!=')} {b}"
        if k < 0.6:
            return f"({a} & {self.ch(1, 3, 7, 15)}) == {self.ch(0, 1, 2)}"
        if k < 0.7:
            return "s.c"
        if k < 0.8:
            return f"is_a(mk_e({a}))"
        return f"{a} % {self.ch(3, 5, 7, 11)} == 0"

    def edge_stmt(self, p, env, lv):
        # raw int registers driven to the edge, optionally one step past it at a chosen iteration
        k = self.r.random()
        if k < 0.25:
            return [f"{p}big = big + 1", f"{p}if big > {self.IMAX - 2} {{ big = {self.IMAX - 50} + (big & 0) }}"]
        if k < 0.45:
            return [f"{p}small = small - 1", f"{p}if small < {self.IMIN + 2} {{ small = {self.IMIN + 50} }}"]
        if k < 0.6:
            return [f"{p}acc = (acc + ((big - {self.IMAX - 100}) * ((small - {int_lit(self.IMIN)}) & 0xFF))) & {M}"]
        if k < 0.7:
            return [f"{p}acc = (acc + ((-small) & 0xFFFF) + ((-big) & 0xFF)) & {M}"]
        if k < 0.8 and self.trap:
            # traps exactly when the global counter reaches trap_at
            return [f"{p}if tick == {self.trap_at} {{ big = big + 9223372036854775807 }}"]
        if k < 0.9 and self.trap:
            return [f"{p}if tick == {self.trap_at} {{ acc = acc + (small * 131072) }}"]
        if self.trap and self.r.random() < 0.5:
            return [f"{p}acc = (acc + (7).shl_div(tick - {self.trap_at}, 3)) & {M}"]
        return [f"{p}acc = (acc + ((big & 0xFFFF) << 31) + (big >> 40)) & {M}"]

    def vec_stmt(self, p, env):
        k = self.r.random()
        x = self.expr(env, 1)
        if k < 0.2:
            return [f"{p}v[({x} & 0xFFFF) % v.len()] = {self.expr(env, 1)} & 0xFFFFF"]
        if k < 0.3:
            return [f"{p}if v.len() < 3000 {{ v.push({x} & 0xFFFF) }}"]
        if k < 0.38:
            return [f"{p}if tick % {self.ch(7, 31, 127)} == 3 {{ grow(v, {self.ch(1, 3, 17)}) }}"]
        if k < 0.45:
            return [f"{p}if tick % {self.ch(13, 61, 251)} == 5 {{ shrink(v) }}"]
        if k < 0.5:
            return [f"{p}if tick % {self.ch(97, 499)} == 7 {{ reset(v) }}"]
        if k < 0.55:
            return [f"{p}if tick % {self.ch(5, 41)} == 1 && v.len() > 2 {{ let _ = v.pop() }}"]
        if k < 0.6:
            return [f"{p}if tick % {self.ch(17, 77)} == 2 {{ w.push({x} & 0xFF) }}"]
        if k < 0.65:
            return [f"{p}if tick % {self.ch(19, 83)} == 4 && v.len() > 4 {{ v.truncate(v.len() - 2) }}"]
        if k < 0.7:
            return [f"{p}if tick % {self.ch(23, 89)} == 6 {{ v.insert(0, {x} & 0xFF) }}"]
        if k < 0.75:
            return [f"{p}if tick % {self.ch(29, 91)} == 8 && v.len() > 3 {{ let _ = v.swap_remove(1) }}"]
        if k < 0.85:
            it = self.fresh("x")
            out = [f"{p}let mut cnt{it} = 0", f"{p}for {it} in w {{"]
            out.append(f"{p}    acc = (acc * 3 + {it} + v.len()) & {M}")
            out.append(f"{p}    cnt{it} = cnt{it} + 1")
            out.append(f"{p}    if cnt{it} % {self.ch(3, 5, 9)} == 1 && v.len() < 3000 {{ v.push(cnt{it} & 0xFF) }}")
            out.append(f"{p}    if cnt{it} % {self.ch(4, 7)} == 2 && v.len() > 3 {{ let _ = v.pop() }}")
            out.append(f"{p}    if cnt{it} > {self.ch(10, 40, 200)} {{ break }}")
            out.append(f"{p}}}")
            return out
        it = self.fresh("y")
        return [f"{p}for {it} in ar {{ acc = (acc + {it} * 3) & {M} }}", f"{p}ar[{x} & 7] = acc & 0xFFFF"]

    def stmt(self, env, depth, lv, ind):
        p = "    " * ind
        k = self.r.random()
        if k < 0.14:
            return [f"{p}{self.ch('a', 'b')} = {self.expr(env)}"]
        if k < 0.26:
            return [f"{p}acc = (acc + {self.expr(env)}) & {M}"]
        if k < 0.38:
            return self.edge_stmt(p, env, lv)
        if k < 0.55:
            return self.vec_stmt(p, env)
        if k < 0.6:
            return [f"{p}s.{self.ch('a', 'd')} = {self.expr(env)} & 0xFFFFF", f"{p}s.b = s.b * 0.5 + 1.0"]
        if k < 0.66:
            return [f"{p}g0 = (g0 + {self.expr(env, 1)}) & 0xFFFFF"]
        if k < 0.69:
            return [f"{p}gwrite({self.expr(env, 1)} & 0xFF)"]
        if k < 0.73:
            e = self.fresh("e")
            return [f"{p}let {e} = mk_e({self.expr(env, 1)})", f"{p}acc = (acc + eval_e({e}) + eval_e(keep)) & {M}",
                    f"{p}if tick % {self.ch(3, 11)} == 0 {{ keep = {e} }}"]
        if k < 0.76:
            t = self.fresh("t")
            return [f"{p}let {t} = ({self.iv(env)}).to_string()", f"{p}acc = (acc + {t}.len() + s.a + v[0]) & {M}"]
        if k < 0.79:
            # rare exit to the interpreter with many raw registers live
            return [f"{p}if tick % {self.ch(997, 4093, 1500)} == {self.ch(1, 2)} {{ let q = \"{{a}}{{big}}\"; acc = (acc + q.len()) & {M} }}"]
        if k < 0.86 and depth < 2:
            out = [f"{p}if {self.cond(env)} {{"]
            for _ in range(self.r.randint(1, 3)):
                out += self.stmt(env, depth + 1, lv, ind + 1)
            if self.r.random() < 0.4:
                out.append(f"{p}}} else {{")
                out += self.stmt(env, depth + 1, lv, ind + 1)
            out.append(f"{p}}}")
            return out
        if k < 0.9 and lv:
            return [f"{p}if {self.cond(env)} {{ continue }}"]
        if k < 0.93 and depth < 2:
            return self.loop(env, depth + 1, lv, ind, n=self.ch(2, 3, 5))
        if k < 0.96:
            return [f"{p}acc = (acc + match keep {{ E::A(x) => x & 0xFF, E::B {{ x, y }} => (x ^ y) & 0xFF, E::C => 3 }}) & {M}"]
        return [f"{p}if g1 > {self.r.randint(5, 60)} {{ acc = (acc + late_read()) & {M} }}"]

    def loop(self, env, depth, lv, ind, n):
        p = "    " * ind
        i = f"i{depth}"
        out = []
        if self.r.random() < 0.6:
            out.append(f"{p}for {i} in 0..{n} {{")
        else:
            out.append(f"{p}let mut w{depth} = 0")
            out.append(f"{p}while w{depth} < {n} {{")
            out.append(f"{p}    let {i} = w{depth}")
            out.append(f"{p}    w{depth} = w{depth} + 1")
        if depth == 0:
            out.append(f"{p}    tick = tick + 1")
            if self.trap:
                out.append(self.r.choice([
                    f"{p}    if tick == {self.trap_at} {{ big = big + 9223372036854775807 }}",
                    f"{p}    if tick == {self.trap_at} {{ acc = acc + (small * 131072) }}",
                    f"{p}    acc = (acc + (7).shl_div(tick - {self.trap_at}, 3)) & {M}",
                ]))
        env2 = env + [i]
        for _ in range(self.r.randint(3, 7)):
            out += self.stmt(env2, depth, lv + [i], ind + 1)
        out.append(f"{p}}}")
        return out

    def program(self):
        r = self.r
        L = ["struct S { a: int, b: float, c: bool, d: int }"]
        L.append("enum E { A(int), B { x: int, y: int }, C }")
        L.append(f"let mut g0 = {r.randint(0, 99)}")
        L.append(f"let mut g1 = {r.randint(0, 9)}")
        L.append("let mut tick = 0")
        L.append("fn fa(x: int) -> int { return (x * 3 + 1) & 0xFFFF }")
        L.append("fn fb(x: int) -> int { return (x ^ 0x55) + g0 }")
        L.append("fn gread() -> int { return (g0 + g1) & 0xFFFF }")
        L.append("fn gwrite(x: int) { g1 = (g1 + x) & 0xFFF }")
        L.append("fn late_read() -> int { return glate + 1 }")
        L.append("fn grow(mut v: Vec<int>, k: int) { for j in 0..k { v.push(j & 0xFF) } }")
        L.append("fn shrink(mut v: Vec<int>) { if v.len() > 4 { v.truncate(v.len() / 2) } }")
        L.append("fn reset(mut v: Vec<int>) { v.clear(); v.push(3); v.push(5) }")
        L.append("fn mk_e(i: int) -> E { let k = i & 3; if k == 0 { return E::A(i & 0xFFF) } if k == 1 { return E::B { x: i & 0xFF, y: (i >> 3) & 0xFF } } return E::C }")
        L.append("fn eval_e(e: E) -> int { match e { E::A(x) => x, E::B { x, y } => x * 2 + y, E::C => 7 } }")
        L.append("fn is_a(e: E) -> bool { match e { E::A(_) => true, _ => false } }")
        L.append("let mut gfun = fa")
        L.append("fn kern(p0: int, p1: int, mut v: Vec<int>, mut w: Vec<int>, mut ar: [int; 8], mut s: S) -> int {")
        L.append("    let mut a = p0")
        L.append("    let mut b = p1 + 1")
        L.append("    let mut acc = 0")
        L.append(f"    let mut big = {self.IMAX - r.choice([5, 300, 70000])}")
        L.append(f"    let mut small = {self.IMIN + r.choice([5, 300, 70000])}")
        L.append("    let mut keep = mk_e(p0)")
        # at the i64 edge big and small stay out of general expressions, which would overflow at once
        env = ["p0", "p1", "a", "b", "acc"] + (["big", "small"] if self.edge == 47 else [])
        n = self.n_inner if not self.long else 1100000
        if self.long:
            # one activation long enough to recompile mid-loop; a light body keeps the interpreter run short
            L.append(f"    for i0 in 0..{n} {{")
            L.append("        tick = tick + 1")
            L.append("        big = big + 1")
            L.append(f"        if big > {self.IMAX - 2} {{ big = {self.IMAX - 1000} }}")
            L.append(f"        acc = (acc + (big & 0xFF) + ((small + i0) & 0xF)) & {M}")
            L.append(f"        if i0 == {r.choice([1048570, 1048577, 1049000, 1099990])} {{ let q = \"{{acc}}\"; acc = (acc + q.len() + v.len()) & {M} }}")
            if self.trap:
                L.append(f"        if i0 == {r.choice([1048576, 1048600, 1099000])} {{ small = small - {r.choice([300, 70000, 5])} }}")
            L.append("    }")
        else:
            L += self.loop(env, 0, [], 1, n)
        L.append("    let keep2 = (p0 & 1) + v.len() + w.len() + ar[0] + s.a + s.d + (big & 0xFF) + (small & 0xFF)")
        L.append(f"    return (acc + a + b + keep2) & {M}")
        L.append("}")
        # declared after the fns that read it, initialised before they run
        L.append("let glate = 40")
        L.append(f"let mut v0: Vec<int> = vec![{', '.join(str(r.randint(0, 99)) for _ in range(r.choice([1, 3, 8])))}]")
        if r.random() < 0.5:
            L.append("let mut w0 = v0")
        else:
            L.append(f"let mut w0: Vec<int> = vec![{', '.join(str(r.randint(0, 99)) for _ in range(r.choice([1, 4])))}]")
        L.append("let mut ar0 = [1, 2, 3, 4, 5, 6, 7, 8]")
        L.append("let mut s0 = S { a: 1, b: 0.5, c: true, d: 2 }")
        L.append("let mut total = 0")
        outer = self.n_outer if not self.long else 2
        L.append(f"for it in 0..{outer} {{")
        L.append("    total = (total + kern(it, total & 255, v0, w0, ar0, s0)) & 0xFFFFFFF")
        L.append(f"    if it % {self.ch(7, 50, 101)} == 3 {{ gfun = fb }}")
        L.append(f"    if it % {self.ch(7, 50, 101)} == 5 {{ gfun = fa }}")
        L.append(f"    if it % {self.ch(30, 77)} == 9 {{ g1 = g1 + 7 }}")
        L.append(f"    if it % {self.ch(100, 250)} == 99 {{ println(\"{{it}} {{total}} {{g0}} {{g1}} {{tick}}\") }}")
        L.append("}")
        L.append("let mut vs = 0")
        L.append("for x in v0 { vs = (vs * 31 + x) & 0xFFFFFFF }")
        L.append("println(\"total={total} g0={g0} g1={g1} tick={tick} vs={vs} vlen={v0.len()} wlen={w0.len()} ar={ar0[0]},{ar0[7]} s={s0.a},{s0.b},{s0.d}\")")
        return "\n".join(L) + "\n"


def generate(rng, index):
    # no oracle: every mode must agree with the others
    return Gen(rng).program(), None
