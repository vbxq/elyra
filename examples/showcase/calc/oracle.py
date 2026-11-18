import math
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent


def fmt(x):
    if x == 0 and math.copysign(1.0, x) < 0:
        return "-0.0"
    r = repr(float(x))
    if "e" in r or "inf" in r or "nan" in r:
        raise SystemExit(f"oracle cannot format {r} like the runtime")
    return r


class Err(Exception):
    pass


def script_lines():
    src = (HERE / "calc.aelys").read_text()
    block = src.split("let script = vec![", 1)[1].split("\n]\n", 1)[0]
    return re.findall(r'^\s*"((?:[^"\\]|\\.)*)",\s*$', block, re.M)


def tokenize(src):
    out = []
    i = 0
    while i < len(src):
        c = src[i]
        col = i + 1
        if c.isspace():
            i += 1
        elif c.isdigit() or c == ".":
            j = i
            while j < len(src) and (src[j].isdigit() or src[j] == "."):
                j += 1
            text = src[i:j]
            if not re.fullmatch(r"\d+(\.\d*)?|\.\d+", text):
                raise Err(f"column {col}: malformed number '{text}'")
            out.append((("num", float(text)), col, text))
            i = j
        elif c.isalpha() or c == "_":
            j = i
            while j < len(src) and (src[j].isalnum() or src[j] == "_"):
                j += 1
            out.append((("id", src[i:j]), col, src[i:j]))
            i = j
        elif c in "+-*/^(),=":
            out.append((("p", c), col, c))
            i += 1
        else:
            raise Err(f"column {col}: unexpected character '{c}'")
    return out


class Parser:
    def __init__(self, toks):
        self.toks = toks
        self.pos = 0

    def peek(self):
        return self.toks[self.pos][0] if self.pos < len(self.toks) else None

    def unexpected(self):
        if self.pos < len(self.toks):
            _, col, text = self.toks[self.pos]
            return Err(f"column {col}: unexpected '{text}'")
        return Err("unexpected end of input")

    def next(self):
        if self.pos >= len(self.toks):
            raise Err("unexpected end of input")
        self.pos += 1
        return self.toks[self.pos - 1]

    def expect(self, p):
        if self.peek() != ("p", p):
            raise self.unexpected()
        self.pos += 1

    def statement(self):
        if self.peek() == ("id", "let"):
            self.pos += 1
            tok, col, text = self.next()
            if tok[0] != "id":
                raise Err(f"column {col}: unexpected '{text}'")
            self.expect("=")
            stmt = ("let", tok[1], self.expr())
        else:
            stmt = ("eval", self.expr())
        if self.pos < len(self.toks):
            raise self.unexpected()
        return stmt

    def expr(self):
        lhs = self.term()
        while self.peek() in (("p", "+"), ("p", "-")):
            op = self.next()[0][1]
            lhs = ("bin", op, lhs, self.term())
        return lhs

    def term(self):
        lhs = self.unary()
        while self.peek() in (("p", "*"), ("p", "/")):
            op = self.next()[0][1]
            lhs = ("bin", op, lhs, self.unary())
        return lhs

    def unary(self):
        if self.peek() == ("p", "-"):
            self.pos += 1
            return ("neg", self.unary())
        base = self.atom()
        if self.peek() == ("p", "^"):
            self.pos += 1
            return ("bin", "^", base, self.unary())
        return base

    def atom(self):
        tok, col, text = self.next()
        if tok[0] == "num":
            return ("num", tok[1])
        if tok[0] == "id":
            if self.peek() == ("p", "("):
                self.pos += 1
                args = []
                if self.peek() == ("p", ")"):
                    self.pos += 1
                    return ("call", tok[1], args)
                args.append(self.expr())
                while self.peek() == ("p", ","):
                    self.pos += 1
                    args.append(self.expr())
                self.expect(")")
                return ("call", tok[1], args)
            return ("var", tok[1])
        if tok == ("p", "("):
            inner = self.expr()
            self.expect(")")
            return inner
        raise Err(f"column {col}: unexpected '{text}'")


ARITY = {"sqrt": 1, "abs": 1, "max": 2, "min": 2}


def evaluate(e, env):
    kind = e[0]
    if kind == "num":
        return e[1]
    if kind == "var":
        if e[1] not in env:
            raise Err(f"unknown variable '{e[1]}'")
        return env[e[1]]
    if kind == "neg":
        return -evaluate(e[1], env)
    if kind == "bin":
        _, op, l, r = e
        a = evaluate(l, env)
        b = evaluate(r, env)
        if op == "+":
            return a + b
        if op == "-":
            return a - b
        if op == "*":
            return a * b
        if op == "/":
            if b == 0.0:
                raise Err("division by zero")
            return a / b
        return math.pow(a, b)
    _, name, args = e
    vals = [evaluate(a, env) for a in args]
    if name not in ARITY:
        raise Err(f"unknown function '{name}'")
    n = ARITY[name]
    if len(vals) != n:
        word = "argument" if n == 1 else "arguments"
        raise Err(f"{name}() takes {n} {word}, got {len(vals)}")
    if name == "sqrt":
        if vals[0] < 0.0:
            raise Err(f"sqrt() is undefined for {fmt(vals[0])}")
        return math.sqrt(vals[0])
    if name == "abs":
        return abs(vals[0])
    if name == "max":
        return max(vals)
    return min(vals)


def main():
    env = {"pi": math.pi}
    out = []
    for line in script_lines():
        out.append(f"> {line}")
        try:
            stmt = Parser(tokenize(line)).statement()
            if stmt[0] == "let":
                v = evaluate(stmt[2], env)
                env[stmt[1]] = v
                out.append(f"{stmt[1]} = {fmt(v)}")
            else:
                out.append(fmt(evaluate(stmt[1], env)))
        except Err as e:
            out.append(f"error: {e}")
    sys.stdout.write("\n".join(out) + "\n")


main()
