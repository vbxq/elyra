import re

DESCRIPTION = "closures capturing loop variables and mutated upvalues across labeled break/continue in nested for/while/loop"

PY_PRELUDE = '''class Cell:
    def __init__(s, v): s.v = v
class ContL0(Exception): pass
class ContL1(Exception): pass
class BrkL0(Exception): pass
class BrkL1(Exception): pass
'''
JUMPS = {"continue 'L0": "ContL0", "continue 'L1": "ContL1", "break 'L1": "BrkL1", "break 'L0": "BrkL0"}


def py_expr(e):
    for nm in ["outer", "b", "i", "j"]:
        e = re.sub(r"\b%s\b" % nm, nm + ".v", e)
    return e


class Gen:
    def __init__(self, rng):
        self.r = rng
        self.k0 = rng.choice(["for", "while", "loop"])
        self.k1 = rng.choice(["for", "while", "loop"])
        self.in_fn = rng.random() < 0.5
        self.A = rng.randint(2, 4)
        self.B = rng.randint(2, 4)
        self.inner = [("c",)]
        for _ in range(rng.randint(2, 6)):
            self.inner.append(rng.choice([("binc",), ("jump", rng.choice(list(JUMPS)), self.cond()), ("push",),
                                          ("push",), ("outer",), ("match",), ("iflet",)]))
        self.after = [rng.choice(["pushb", "binc3", "none"]) for _ in range(rng.randint(0, 2))]

    def cond(self):
        r = self.r
        return r.choice([f"(iv + jv) % {r.randint(2, 4)} == {r.randint(0, 1)}", f"jv == {r.randint(0, 3)}",
                         f"iv * jv > {r.randint(1, 6)}", f"b % 3 == {r.randint(0, 2)}", "jv == iv"])

    def expr(self, avail):
        r = self.r
        terms = r.sample(avail, k=min(len(avail), r.randint(1, 3)))
        return " + ".join(f"{t} * {r.randint(1, 9)}" for t in terms)

    def emit(self):
        el, py = [], []

        def E(d, t):
            el.append("    " * d + t)

        def P(d, t):
            py.append("    " * d + t)

        base = 1 if self.in_fn else 0
        if self.in_fn:
            E(0, "fn run() -> string {")
        P(0, "def run():")
        P(1, "fs = []")
        P(1, "outer = Cell(0)")
        E(base, "let mut fs: Vec<fn() -> int> = vec![]")
        E(base, "let mut outer = 0")
        d = base
        if self.k0 == "for":
            E(d, f"'L0: for iv in 0..{self.A} {{")
            P(1, f"for iv in range({self.A}):")
            P(2, "try:")
        else:
            E(d, "let mut i = 0")
            P(1, "i = Cell(0)")
            E(d, f"'L0: while i < {self.A} {{" if self.k0 == "while" else "'L0: loop {")
            P(1, "while True:")
            P(2, "try:")
            if self.k0 == "loop":
                E(d + 1, f"if i >= {self.A} {{ break }}")
            P(3, f"if i.v >= {self.A}: break")
            E(d + 1, "let iv = i")
            E(d + 1, "i += 1")
            P(3, "iv = i.v")
            P(3, "i.v += 1")
        d += 1
        pd = 3
        E(d, "let mut b = iv * 10")
        P(pd, "b = Cell(iv * 10)")
        if self.k1 == "for":
            E(d, f"'L1: for jv in 0..{self.B} {{")
            P(pd, f"for jv in range({self.B}):")
            P(pd + 1, "try:")
        else:
            E(d, "let mut j = 0")
            P(pd, "j = Cell(0)")
            E(d, f"'L1: while j < {self.B} {{" if self.k1 == "while" else "'L1: loop {")
            P(pd, "while True:")
            P(pd + 1, "try:")
            if self.k1 == "loop":
                E(d + 1, f"if j >= {self.B} {{ break }}")
            P(pd + 2, f"if j.v >= {self.B}: break")
            E(d + 1, "let jv = j")
            E(d + 1, "j += 1")
            P(pd + 2, "jv = j.v")
            P(pd + 2, "j.v += 1")
        d += 1
        q = pd + 2
        avail = ["iv", "jv", "b", "c", "outer"] + (["i"] if self.k0 != "for" else []) + (["j"] if self.k1 != "for" else [])
        captures = "iv=iv, jv=jv, c=c, b=b, outer=outer" + (", i=i" if "i" in avail else "") + (", j=j" if "j" in avail else "")
        for st in self.inner:
            k = st[0]
            if k == "c":
                E(d, "let c = jv + b")
                P(q, "c = jv + b.v")
            elif k == "binc":
                E(d, "b += 1")
                P(q, "b.v += 1")
            elif k == "outer":
                E(d, "outer += 1")
                P(q, "outer.v += 1")
            elif k == "jump":
                E(d, f"if {st[2]} {{ {st[1]} }}")
                P(q, f"if {py_expr(st[2])}: raise {JUMPS[st[1]]}()")
            elif k == "push":
                e = self.expr(avail)
                E(d, f"fs.push(fn() -> int {{ {e} }})")
                P(q, f"fs.append(lambda {captures}: {py_expr(e)})")
            elif k == "match":
                m = self.r.randint(2, 5)
                E(d, f"match Some(jv * {m}) {{")
                E(d + 1, "Some(v) => fs.push(fn() -> int { v + b }),")
                E(d + 1, "None => {}")
                E(d, "}")
                P(q, f"v = jv * {m}")
                P(q, "fs.append(lambda v=v, b=b: v + b.v)")
            elif k == "iflet":
                E(d, "if let Some(v) = Some(c + 1) { fs.push(fn() -> int { v * 100 + outer }) }")
                P(q, "v = c + 1")
                P(q, "fs.append(lambda v=v, outer=outer: v * 100 + outer.v)")
        d -= 1
        E(d, "}")
        P(pd + 1, "except ContL1: continue")
        P(pd + 1, "except BrkL1: break")
        for st in self.after:
            if st == "pushb":
                E(d, "fs.push(fn() -> int { b * 7 + iv })")
                P(pd, "fs.append(lambda b=b, iv=iv: b.v * 7 + iv)")
            elif st == "binc3":
                E(d, "b += 3")
                P(pd, "b.v += 3")
        d -= 1
        E(d, "}")
        P(2, "except ContL0: continue")
        P(2, "except BrkL0: break")
        E(d, "outer += 1000")
        P(1, "outer.v += 1000")
        E(d, 'let mut acc = ""')
        E(d, 'for f in fs { acc += "{f()}," }')
        P(1, 'return "".join(f"{f()}," for f in fs)')
        if self.in_fn:
            E(d, "acc")
            E(0, "}")
            E(0, "println(run())")
        else:
            E(d, "println(acc)")
        return "\n".join(el) + "\n", PY_PRELUDE + "\n".join(py) + "\n"


def generate(rng, index):
    src, py = Gen(rng).emit()
    ns = {}
    exec(py, ns)
    return src, ns["run"]() + "\n"
