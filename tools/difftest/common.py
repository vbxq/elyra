I64MIN = -(1 << 63)
I64MAX = (1 << 63) - 1

# expected value for a program that must stop on an integer trap (overflow or division by zero)
TRAP = "<trap>"


class Trap(Exception):
    pass


def chk(v):
    if v < I64MIN or v > I64MAX:
        raise Trap()
    return v


def tdiv(a, b):
    if b == 0:
        raise Trap()
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b >= 0) else -q


def trem(a, b):
    if b == 0:
        raise Trap()
    return a - b * tdiv(a, b)


def int_lit(v):
    if v == I64MIN:
        return "(-9223372036854775807 - 1)"
    return f"({v})" if v < 0 else str(v)


def wrap_bits(v, bits, signed):
    v &= (1 << bits) - 1
    if signed and v >> (bits - 1):
        v -= 1 << bits
    return v


def as_main(stmts, use_main):
    if not use_main:
        return stmts + "\n"
    body = "\n".join("    " + line if line else line for line in stmts.split("\n"))
    return "fn main() {\n" + body + "\n}\nmain()\n"


def lines(*xs):
    return "".join(f"{x}\n" for x in xs)
