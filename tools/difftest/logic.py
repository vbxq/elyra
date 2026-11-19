from common import as_main

DESCRIPTION = "&& || ! and boolean ==/!= with logged side effects and minimal parentheses, in many syntactic positions"

PREC = {"||": 1, "&&": 2, "==": 6, "!=": 6, "<": 7}
CMP = {"<": lambda a, b: a < b, "<=": lambda a, b: a <= b, ">": lambda a, b: a > b, ">=": lambda a, b: a >= b,
       "==": lambda a, b: a == b, "!=": lambda a, b: a != b}


def neg_lit(v):
    return str(v) if v >= 0 else f"(0 - {-v})"


def expr(rng, d):
    r = rng.random()
    if d == 0 or r < 0.25:
        c = rng.random()
        if c < 0.35:
            k, v = rng.randint(0, 9), rng.random() < 0.5
            return f'{"t" if v else "f"}({k})', 12, (lambda log, k=k, v=v: (log.append(k), v)[1])
        if c < 0.5:
            v = rng.random() < 0.5
            return ("true" if v else "false"), 12, (lambda log, v=v: v)
        if c < 0.75:
            a, b = rng.randint(-3, 3), rng.randint(-3, 3)
            op = rng.choice(list(CMP))
            res = CMP[op](a, b)
            sa = neg_lit(a)
            if rng.random() < 0.3:
                sa = f"{a - 1} + 1" if a - 1 >= 0 else f"(0 - {1 - a}) + 1"
            return f"{sa} {op} {neg_lit(b)}", 7, (lambda log, res=res: res)
        k = rng.randint(0, 3)
        return f"bv[{k}]", 12, (lambda log, k=k: k % 2 == 0)
    if r < 0.4:
        s, p, f = expr(rng, d - 1)
        if p < 11:
            s = f"({s})"
        return f"!{s}", 11, (lambda log: not f(log))
    if r < 0.47:
        a, pa, fa = expr(rng, d - 1)
        b, pb, fb = expr(rng, d - 1)
        op = rng.choice(["==", "!="])
        if pa <= 7:
            a = f"({a})"
        if pb <= 7:
            b = f"({b})"
        if op == "==":
            return f"{a} == {b}", 6, (lambda log: fa(log) == fb(log))
        return f"{a} != {b}", 6, (lambda log: fa(log) != fb(log))
    op = rng.choice(["&&", "||"])
    a, pa, fa = expr(rng, d - 1)
    b, pb, fb = expr(rng, d - 1)
    pr = PREC[op]
    if pa < pr:
        a = f"({a})"
    if pb < pr or (pb == pr and rng.random() < 0.7):
        b = f"({b})"
    if op == "&&":
        return f"{a} && {b}", pr, (lambda log: fa(log) and fb(log))
    return f"{a} || {b}", pr, (lambda log: fa(log) or fb(log))


HEAD = '''let mut LOG = ""
let bv = vec![true, false, true, false]
fn t(k: int) -> bool { LOG = LOG + "{k}"; true }
fn f(k: int) -> bool { LOG = LOG + "{k}"; false }
struct Hold { b: bool, n: int }
fn ident(b: bool) -> bool { b }
fn ret(x: int) -> bool { return EXPR }
'''

POSITIONS = {
    "let": "let r = EXPR",
    "if": "let r = if EXPR { true } else { false }",
    "while": "let mut r = false\nlet mut w = 0\nwhile w == 0 && (EXPR) { r = true; w = 1 }\nw = 2",
    "guard": "let r = match 5 { n if EXPR => true, _ => false }",
    "closure": "let c = fn() -> bool { EXPR }\nlet r = c()",
    "fmt": 'let s = "{EXPR}"\nlet r = s == "true"',
    "arg": "let r = ident(EXPR)",
    "ret": "let r = ret(0)",
    "field": "let h = Hold { b: EXPR, n: 1 }\nlet r = h.b",
    "vec": "let vv = vec![EXPR, true]\nlet r = vv[0]",
    "assign": "let mut r = false\nr = EXPR",
    "tuple": "let tp = (1, EXPR)\nlet r = tp.1",
    "not_call": "let r = !ident(!(EXPR))",
}


def generate(rng, index):
    s, _, fn = expr(rng, rng.randint(1, 4))
    pos = rng.choice(list(POSITIONS))
    log = []
    v = fn(log)
    head = HEAD.replace("EXPR", s if pos == "ret" else "true")
    body = POSITIONS[pos].replace("EXPR", s)
    code = head + as_main(body + "\nprintln(r)\nprintln(LOG)", rng.random() < 0.5)
    return code, f'{"true" if v else "false"}\n{"".join(map(str, log))}\n'
