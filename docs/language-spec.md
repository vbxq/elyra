# Language Specification

This is the complete reference for Aelys syntax and semantics. If you're new to the language, start with [Getting Started](getting-started.md) instead.

## Lexical Structure

### Comments

```rust
// single line comment
```

Block comments may span lines:

```rust
/* an invariant that spans one line or more */
```

### Identifiers

Must start with a letter or underscore, then letters, digits, or underscores:

```
foo
_private
camelCase
snake_case
MAX_VALUE
Thing2
```

A name that starts with two underscores (`__clone`, `__tostring`) belongs to
the compiler and its builtins. Calling a builtin such as `__tostring(x)` is
allowed, but declaring such a name (a variable, constant, function, parameter,
pattern or loop binding, type, field, variant, type parameter or module alias)
is E0114.

### Reserved Words

```
let mut fn if else while for in return break continue match struct enum
trait impl and or not pub needs as from true false null
```

`null` is reserved only so that it can be rejected; see [Booleans](#literals) below.

`loop` is contextual: it opens a loop only when a `{` follows it, so `let loop = 1`
still declares a variable. `step` is contextual too: it is a keyword only after
the range of a `for` header, so a function, variable or field may be named `step`.
A label is an identifier written after a quote, `'outer`, and is only meaningful
before `for`, `while` or `loop` and after `break` or `continue`. A quote, one
character and a closing quote (`'a'`) is a char literal, not a label, and a quote,
a longer name and a closing quote (`'ab'`) is an invalid char literal.

`where`, `dyn`, `self` and `const` are contextual. They carry meaning in a
generic bound, a type position, a method receiver and a constant declaration
(`const NAME = ...`) respectively, and they are still ordinary identifiers
everywhere else:

```rust
let where = 1
let dyn = 2
let self = 3
println(where + dyn + self)
```

### Literals

**Integers**
```rust
42
-17
0
1_000_000      // underscores for readability
0xFF           // hexadecimal
0b1010         // binary
0o755          // octal
```

An integer literal is an `int`, a 48-bit signed integer (roughly +-140 trillion), unless its
context expects a sized integer type (see [Integer types](#integer-types)). The 48-bit width
comes from NaN-boxing: the VM packs type information into the unused bits of IEEE 754 NaN
values. A literal outside the 48-bit range is E0209.

Underscores are ignored, so `1_000_000` is just `1000000`. Handy for big numbers or binary patterns like `0b1111_0000`.

**Floats**
```rust
3.14
-0.5
1.0
2.5e10
```

Standard IEEE 754 double precision (64-bit).

**Strings**
```rust
"hello"
"line1\nline2"
"tab\there"
"quote: \""
"backslash: \\"
```

UTF-8 encoded. Escape sequences: `\n` (newline), `\t` (tab), `\r` (carriage return), `\\` (backslash), `\"` (quote), `\'` (single quote), `\0` (NUL), and `\u{...}` with one to six hex digits naming a Unicode scalar value (`"caf\u{e9}"` is `"café"`).

**Characters**
```rust
'a'
'\n'
'\''
'\u{e9}'
```

A `char` literal holds exactly one Unicode scalar value between single quotes and
takes the same escapes as a string. An empty literal, a literal with two or more
characters, a missing closing quote, and a `\u{...}` escape that names no scalar
value (above `10FFFF`, or a surrogate in `D800`-`DFFF`) are each E0008.

**String Interpolation**

Embed expressions directly in strings using `{expression}`:

```rust
let name = "Kaito"
let age = 67
"Hello, {name}! You are {age} years old."  // "Hello, Kaito! You are 67 years old."
```

Any expression works inside the braces:

```rust
let x = 10
"x + 5 = {x + 5}"           // "x + 5 = 15"
"doubled: {x * 2}"          // "doubled: 20"
```

Values are converted to strings automatically. To include a literal brace, double it:

```rust
"JSON: {{key}}"             // "JSON: {key}"
```

**Placeholder Syntax**

You can also use `{}` as placeholders filled by function call arguments:

```rust
print("Hello, {}!", "world")     // "Hello, world!"
print("x={}, y={}", 10, 20)      // "x=10, y=20"
```

Placeholders are filled left-to-right. You can mix inline expressions and placeholders:

```rust
let name = "Reimu"
print("Hi {name}, your number is {}", 42)
```

**Booleans**
```rust
true
false
```

There is no null value in the Aelys surface language. Use `Option<T>` for absence
and `Result<T, E>` for recoverable failure. The old `null` spelling is rejected
with a diagnostic that names these replacements.

## Types

| Type | Description | Size |
|------|-------------|------|
| `int` | Signed integer, overflow traps | 48-bit |
| `i8` `i16` `i32` | Signed integer, wrapping | 8, 16, 32-bit |
| `u8` `u16` `u32` | Unsigned integer, wrapping | 8, 16, 32-bit |
| `float` | Floating point | 64-bit |
| `string` | UTF-8 text | heap allocated |
| `char` | One Unicode scalar value | immediate |
| `bool` | Boolean | 1 bit (packed) |
| `unit` | No meaningful value | immediate |
| `Option<T>` | A value or absence | heap allocated when needed |
| `Result<T, E>` | Success or failure | heap allocated when needed |
| `Error` | Structured error value | heap allocated when needed |
| `fn(T, ...) -> R` | Function/closure | heap allocated |
| `Name { field: T, ... }` | Fixed-layout struct | heap allocated |
| `[T; N]` | Fixed-size array | heap allocated |
| `Vec<T>` | Growable vector | heap allocated |
| `Map<K, V>` | Insertion-ordered hash map | heap allocated |
| `(A, B, ...)` | Immutable tuple of 2 to 8 elements | heap allocated |

### Integer types

`int` is the default integer. Its arithmetic is checked: `+`, `-`, `*`, unary `-`, `<<`
and an `as` conversion trap with `integer overflow outside the supported 48-bit range` when
the exact result leaves the 48-bit range, and `/` or `%` by zero traps with
`division by zero`. `int` has no other width; `i64`, `int64`, `u64` and `uint64` are E0461
(`i64 is not available: int is a 48-bit integer; use int, or i32/u32 for wrapping
arithmetic`).

The sized types `i8`, `i16`, `i32`, `u8`, `u16` and `u32` wrap. Every operator on them
computes the exact result and keeps its low 8, 16 or 32 bits as two's complement, so the
value always lies in the type's range, in a variable, a struct field, an array or a `Vec`:

```rust
let a: u32 = 0xFFFFFFFF
let one: u32 = 1
println(a + one)          // 0
let b: i32 = 0x7FFFFFFF
println(b + 1)            // -2147483648
let c: u8 = 200
println(c + c)            // 144
```

- `+ - * & | ^ ~` and unary `-` wrap to the width.
- `/` truncates toward zero and `%` takes the sign of the dividend, as in C. Both trap on a
  zero divisor. `i32::MIN / -1` wraps to `i32::MIN` and `i32::MIN % -1` is `0` (likewise
  for `i8` and `i16`).
- `<<` keeps the low bits. `>>` is arithmetic on a signed type and logical on an unsigned
  one. The shift count may have any integer type, `int` included, and a literal count is
  accepted whatever its value; it is taken modulo the width of the shifted value (`& 7`,
  `& 15`, `& 31`, and `& 63` for `int`), so `x << 33` on an `i32` is `x << 1` and a count of
  `-1` is the width minus one. The result has the type of the shifted value.
- Comparisons compare the values.

A binary operator takes two operands of the same integer type. Mixing two integer types,
`int` included, is E0460 `mismatched integer types in '+': i32 and int` with the help line
``convert with `as` ``. A shift count is the exception: `x >> n` with `x: u32` and
`n: int` or `n: u8` is accepted.

An integer literal takes the sized type its context expects: a `let` annotation, a struct
field, an enum constructor payload (`E::A(200)` with `A(u8)`, `E::B { x: -5 }` with an
`i16` field), a function argument, a return value or a function's last expression, an
element of an array or `Vec` literal whose type is known, a `push` onto a `Vec<T>`, the
element, key or value argument of the other `Vec`, array and `Map` methods
(`v.extend(vec![255])`, `m.insert(1, 2)` on a `Map<u8, i32>`), an assignment to a
variable, field, element or `Map` entry, the other operand of a binary operator, a
compound assignment, a `match` pattern on a sized value (a literal, a range bound or a
`const` naming an integer), a branch of an `if` or a `match` arm whose sibling branch is
sized, the last expression of a value block (`{ ...; 200 }`, a multi-line `if` branch or a
block `match` arm) and the `break` values of a `loop`, all wherever the block, `if`,
`match` or `loop` itself has a sized context, and the bounds of a `for` range whose other bound is
sized (the loop variable then has that type). The shifted value of `1 << n` takes the
context's type; the count keeps its own. A literal that does not fit its type is E0301
`integer literal 256 does not fit in u8`. The payload of a generic struct or enum takes
the type its arguments name, written out (`Holder::<u8>::Full(200)`, `Box::<i8> { value:
100 }`) or expected by a `let` annotation, a return type or a parameter (`let b: Box<u8> =
Box { value: 200 }`). A method argument takes the type the receiver's arguments give its
parameter (`s.push(200)` on a `Stack<u8>`). A literal passed to a generic parameter
otherwise stays `int`; the other operand of a binary operator whose type is still being
inferred gives the literal that type once inference settles it.

```rust
let x: i32 = 5
let y: i32 = 3
let p: i32 = (x * y) >> 16          // the 16 is an i32
struct Pixel { v: u8 }
let px = Pixel { v: 1 }
let bytes: Vec<u8> = vec![1, 2, 255]
let pair: [i32; 2] = [1, -1]
```

Any integer type, sized or not, indexes an array or a `Vec`.

#### Conversions with `as`

`as` converts between every integer type, `float` and `bool`:

| From | To | Result |
|------|----|--------|
| any integer | a sized integer | the low bits, two's complement: `3000000000 as i32` is `-1294967296`, `-1 as u8` is `255` |
| any integer | `int` | the same value (every sized value fits) |
| `float` | `int` | truncated toward zero; NaN, an infinity or a value outside 48 bits traps with the overflow error |
| `float` | a sized integer | truncated toward zero, then wrapped like an integer (`-2.7 as u8` is `254`); NaN or an infinity traps with `cannot convert NaN to u8` (or `inf`, `-inf`, and the target type) |
| integer | `float` | the nearest float |
| `bool` | integer | `0` or `1` |

#### Fixed-point helpers

`i32` and `int` have two methods with an exact intermediate, the `FixedMul` and
`FixedDiv` of 16.16 fixed-point code:

- `a.mul_shr(b, n)` is `(a * b) >> n` computed without overflow, then wrapped to `i32`
  (or checked against 48 bits for `int`, trapping on overflow).
- `a.shl_div(b, n)` is `(a << n) / b` computed without overflow and truncated toward zero,
  then wrapped to `i32` (or checked for `int`). A zero `b` traps with `division by zero`.

`b` has the receiver's type and `n` is an `int`. A literal `n` outside 0..=63 is E0462
`shift count 64 of 'mul_shr' is outside 0..=63`; a computed `n` is taken modulo 64
(`& 63`), so 64 shifts by 0 and -1 by 63.

```rust
fn fixed_mul(a: i32, b: i32) -> i32 { a.mul_shr(b, 16) }
fn fixed_div(a: i32, b: i32) -> i32 { a.shl_div(b, 16) }
println(fixed_mul(0x7FFF0000, 0x20000))   // -131072
println(fixed_div(10 << 16, 4 << 16))     // 163840, 2.5 in 16.16
```

Each sized operator, and each of these two methods, is a single VM instruction. The JIT
does not compile a function that uses them; it runs in the interpreter with the same
results.

### Type Annotations

Optional wherever inference can reach a concrete type on its own, which is the
common case. An annotation becomes necessary when nothing in the program pins a
type down: a function whose parameters are unannotated and which is never
called leaves those parameters open, and an open type at the end of inference is
E0353 `a type here stayed unresolved after inference`. The same function is
accepted once a call site supplies the types.

```rust
fn add(a, b) { a + b }
println(add(1, 2))
```

Variables:
```rust
let x: int = 42
let mut name: string = "Reimu"
```

Function parameters and return:
```rust
fn process(input: string, count: int) -> bool {
    return input.len() == count
}
```

Lambdas, and the annotation for a value holding one:
```rust
let f = fn(x: int) -> int { x * 2 }
let g: fn(int) -> int = f
```

A function type is written `fn(T, ...) -> R`, with the parameter types and no
parameter names. A parameter of aggregate type written `mut T` is a `mut`
parameter, which is part of the type (see [Mutable Parameters](#mutable-parameters)).
`function` is not a type: `let f: function`, `fn f(g: function)`
and `fn f() -> function` are each E0372 `unknown type 'function'`.

### Type Inference

The type system uses Hindley-Milner inference. When you write:

```rust
let x = 42
let y = x + 10
```

The compiler knows `x` is `int` (from the literal) and `y` is `int` (from the `+` operation).

Inference supplies types for ordinary expressions. `dynamic` is not part of the
surface language and there is no place where you may write it. Every `dynamic`
annotation is rejected with E0347 `dynamic is not part of Aelys; use a concrete
type or an explicit enum`, whether it appears on a variable, a parameter, or a
return type. Model a value whose shape varies with an enum whose variants name
the cases you accept.

## Variables

### Declaration

```rust
let x = 10          // immutable
let mut y = 20      // mutable
```

### Shadowing

You can redeclare variables in the same scope:

```rust
let x = 10
let x = "now a string"  // shadows previous x
```

Inner scopes can also shadow:

```rust
let x = 1
if true {
    let x = 2    // different x
    println(x)  // 2
}
println(x)      // 1
```

### Scope

Block-scoped. Variables live until their enclosing `}`.

### Constants

`const NAME: T = expr` declares a compile-time constant; `pub const` exports it
from a module. The type annotation is optional and follows the rules of `let`
(`const SMALL: u8 = 200` is a `u8`).

```rust
const SCREEN_W: int = 320
const SCREEN_H = SCREEN_W * 5 / 8      // 200
const MASK = (1 << 16) - 1
const TITLE: string = "doom"
pub const FRACUNIT: int = 1 << 16

let column = [0; SCREEN_H]             // a repeat count
struct Frame { pixels: [int; SCREEN_W] } // an array length in a type
```

The initializer must be a constant expression: integer, float, bool and string
literals; unary `-`, `~` and `not`; arithmetic, shifts and bitwise operators on
integers; arithmetic on floats; comparisons; `and`/`or` on bools; string `+`;
`as` between the integer types and `float`; and other constants, including another
module's `pub const` (`cfg::W`) and associated constants (`Cfg::N`). The value is
computed with the runtime's rules for the constant's type: a sized constant wraps
(`const B: u8 = 200 + 100` is `44`, `const M: u32 = 0xFFFFFFFF + 1` is `0`), each
operand takes the type of the constants it names, and a shift count is taken modulo
the width (`const Z = 1 >> -1` is `0`). Anything else, a function call, a `let`
binding, a collection, a division by zero or an `int` result outside the 48-bit
range, is E0395; for a division or remainder by zero or an overflow, the message
names that cause (`cannot be evaluated at compile time: division by zero`).
Declaring the same constant twice in one scope is E0396.

A constant is folded when the program is compiled: every use is replaced by its
value, so a constant can be read before its declaration line, from a function
declared earlier, and inside closures. A constant can also be declared inside a
function body. It cannot be assigned (E0203), and a local `let` of the same name
shadows it like any other binding.

Across modules, `needs cfg` makes `cfg::W` and `W` available, `needs cfg::W`
makes `W` available, and both forms work as repeat counts and array lengths. A
constant without `pub` stays private (E0313).

## Functions

### Declaration

```rust
fn untyped(param1, param2) {
    param1 + param2
}

fn typed(a: int, b: int) -> int {
    return a + b
}

println(untyped(1, 2))
println(typed(3, 4))
```

The unannotated form takes its parameter types from its call sites. Without a
call site it is E0353; see [Type Annotations](#type-annotations).

### Mutable Parameters

By default, function parameters are immutable. If you try to reassign one, the compiler will complain:

```rust
fn process(buffer: string) -> string {
    buffer += "-"  // ✗ error: buffer is immutable
    return buffer
}
```

Add `mut` before the parameter name to allow reassignment:

```rust
fn process(mut buffer: string) -> string {
    buffer += "-"  // ✓ works
    return buffer
}
```

This is especially useful in loops where you accumulate a result:

```rust
fn build_line(mut acc: int, n: int) -> int {
    for i in 0..n {
        acc++
    }
    return acc
}

build_line(10, 5)  // 15
```

Reassigning a mutable parameter never affects the caller: the parameter is a
new binding.

```rust
fn try_modify(mut x: int) -> int {
    x += 100
    return x
}

let a = 1
let b = try_modify(a)  // b = 101
a                       // still 1
```

Aggregates (structs, enums, arrays and `Vec`) are passed by reference, so a
`mut` parameter of aggregate type mutates the caller's value. That is only
allowed when the argument is reachable from a mutable root: passing `a`, `a.f`
or `a[i]` to a `mut` parameter when `a` is immutable is E0323. Pass
`a.clone()` to give the function its own copy. The same rule applies to a
`mut self` receiver (E0331). See [Structs](#structs).

```rust
fn grow(mut v: Vec<int>) { v.push(0) }

let mut shared = vec![1]
grow(shared)                // shared.len() == 2
let fixed = vec![1]
grow(fixed)                 // ✗ error[E0323]: cannot pass `fixed` to the `mut` parameter `v`
grow(fixed.clone())         // ✓ fixed is unchanged
```

A `mut` parameter of aggregate type is part of the function's type. `grow`
has type `fn(mut Vec<int>) -> unit`, a lambda `fn(mut v: Vec<int>) { ... }` has
the same type, and the annotation `fn(mut T, U) -> R` writes it. `fn(mut S) -> R`
and `fn(S) -> R` are different types: neither is accepted where the other is
expected (E0301, with a help line naming the `mut`). An unannotated function
parameter, as `f` in `fn apply(f, s) { f(s) }`, takes the type of the function
passed to it, `mut` included; like its other parameter types it is fixed by the
call sites, so passing it both a `fn(S)` and a `fn(mut S)` is E0301.

`mut` on a parameter whose type is a scalar (`int`, a sized integer, `float`,
`bool`, `string`, `unit`) only rebinds a local copy, which no caller can see, so
it is not part of the type: `fn(mut x: int) -> int` has type `fn(int) -> int`.

A call through a function value checks the `mut` positions of its type exactly
like a direct call: an argument at a `mut` position must have a mutable root or
be a clone, and the other positions accept any argument. This holds for a
function-typed parameter or field, a `let` or `let mut` binding, an element of a
`Vec` or array, a captured value and the result of a call.

```rust
fn show(v: Vec<int>) { println(v.len()) }
fn apply(f: fn(Vec<int>) -> unit, v: Vec<int>) { f(v) }                 // ✓ no mut position
apply(show, fixed)                                                     // ✓
apply(grow, shared)                                                    // ✗ error[E0301]: expected fn(Vec<int>) -> unit, found fn(mut Vec<int>) -> unit
fn grow_all(f: fn(mut Vec<int>) -> unit, v: Vec<int>) { f(v) }         // ✗ error[E0323]: cannot pass `v` to the `mut` parameter 1 of a function value of type `fn(mut Vec<int>) -> unit`
fn grow_all(f: fn(mut Vec<int>) -> unit, mut v: Vec<int>) { f(v) }     // ✓ callers pass a mutable root
let h = grow
h(fixed)                                                               // ✗ error[E0323]: cannot pass `fixed` to the `mut` parameter 1 of a function value of type `fn(mut Vec<int>) -> unit`
```

The callbacks of the built-in collection, `Option` and `Result` methods (`map`,
`filter`, `fold`, `and_then`, ...) accept either form.

Works with lambdas too:

```rust
let add = fn(mut x: int, y: int) -> int {
    x += y
    x
}
```

### Return

Explicit:
```rust
fn foo() -> int {
    return 42
}
```

Implicit (last expression):
```rust
fn foo() -> int {
    42
}
```

Functions without a returned value have type `unit`. A function with a concrete
return type must return that type on every reachable path.

In a function or lambda without a return annotation, a bare `return` (also
directly before `}`) and `return ()` return `unit`, and a trailing assignment
(`x = v`, `x += v`, `s.f = v`, `v[i] = v`, an `if`/`else` or a `match` whose
branches all end in an assignment) is a statement, not a returned value: such a
function has type `unit` and returns `()` at run time. With an explicit return
type, a trailing assignment still yields the assigned value.

An unannotated function that returns a value with `return v` takes the type of
`v`, so every path must yield a value: ending on a trailing assignment (or any
other statement) after such a `return` is E0215.

```rust
let mut g = 0
fn f(c: bool) {
    if c { return 7 }
    g = 5          // E0215: this path falls through without a value
}
```

Every top-level `fn` and `impl` of a file is bound before the file's top-level
code runs, so a top-level `let` initializer may call a function declared further
down the same file or module. Top-level `let` bindings still run in source order:
a function that reads a global whose `let` has not run yet stops the program with
the runtime error ``global `b` read before its initialiser ran``, pointing at the
reading line. The value is never read as a default.

```rust
let a = f()        // runtime error: global `b` read before its initialiser ran
let b = 10
fn f() -> int { b + 1 }
```

### Lambdas (Anonymous Functions)

```rust
let add = fn(a, b) { a + b }
let square = fn(x: int) -> int { x * x }

println(add(1, 2))
println(square(4))
```

A lambda whose parameters are unannotated draws them from its call sites, on the
same terms as an unannotated `fn`; with no call site it is E0353.

### Closures

Functions capture variables from their enclosing scope:

```rust
fn make_counter() {
    let mut count = 0
    return fn() -> int {
        count++
        return count
    }
}

let counter = make_counter()
counter()  // 1
counter()  // 2
counter()  // 3
```

The inner function holds a reference to `count`, which persists across calls.

### Higher-Order Functions

Functions are first-class values:

```rust
fn apply_twice(f, x) {
    return f(f(x))
}

fn double(n) { n * 2 }

apply_twice(double, 5)  // 20
```

## Operators

### Arithmetic

| Operator | Description |
|----------|-------------|
| `+` | Addition (and string concatenation) |
| `-` | Subtraction |
| `*` | Multiplication |
| `/` | Division |
| `%` | Modulo |

Integer division truncates toward zero: `7 / 2` gives `3` and `-7 / 2` gives `-3`; `%` takes
the sign of the dividend (`-7 % 2` is `-1`). On `int` an overflowing result traps; on a sized
integer type it wraps (see [Integer types](#integer-types)). Use floats if you need decimal
results.

### Compound Assignment

Instead of writing `x = x + 1`, you can use compound assignment operators:

| Operator | Equivalent |
|----------|------------|
| `x += y` | `x = x + y` |
| `x -= y` | `x = x - y` |
| `x *= y` | `x = x * y` |
| `x /= y` | `x = x / y` |
| `x %= y` | `x = x % y` |

These work on mutable variables, mutable parameters, and array/vec indices:

```rust
let mut total = 0
total += 10         // 10

let mut scores = [10, 20, 30]
scores[1] += 5      // scores[1] is now 25

let mut s = "hello"
s += " world"       // "hello world"
```

The target is evaluated once: in `ms[pick()].hp -= 10` or `w.ps[i + f()].x *= 2`, every index expression of the place is evaluated exactly once, left to right, before the right-hand side; the same slot is read and written.

### Increment and Decrement

For the common case of adding or subtracting 1:

| Operator | Equivalent |
|----------|------------|
| `x++` | `x = x + 1` |
| `x--` | `x = x - 1` |

```rust
let mut count = 0
count++     // 1
count++     // 2
count--     // 1
```

These are postfix operators. They work on every place a compound assignment
accepts: a variable, a field, an element and nested paths (`s.v++`, `a[0]--`,
`w.ps[pick()].h++`), with each index expression evaluated once. On any other
expression, such as `f()++`, they are E0104.

### Comparison

| Operator | Description |
|----------|-------------|
| `==` | Equal |
| `!=` | Not equal |
| `<` | Less than |
| `<=` | Less than or equal |
| `>` | Greater than |
| `>=` | Greater than or equal |

The ordering operators accept two operands of one numeric type, two `char`
values (ordered by scalar value), or two strings (ordered lexicographically by
scalar value, so `"Z" < "a"`, `"ab" < "abc"` and `"é" > "z"`). A `char` and a
`string` never compare with each other.

### Equality on aggregates

`==` and `!=` are structural on every aggregate the language builds, and the
comparison is deep rather than one level. Two arrays are equal when they have the
same length and every element pair is equal; two Vecs likewise; two structs when
they are the same struct and every field pair is equal; two enum values when they
carry the same variant and every payload slot pair is equal. `Option` and
`Result` follow the same rule because they are enums.

```rust
struct Point { x: int, y: string }
enum Shape { Dot(int), Box { side: int } }

println([1, 2] == [1, 2])                                     // true
println([1, 2] == [1, 3])                                     // false
println(vec![1] == vec![1])                                   // true
println(Some(1) == Some(1))                                   // true
println(Point { x: 1, y: "a" } == Point { x: 1, y: "a" })     // true
println(Point { x: 1, y: "a" } == Point { x: 1, y: "b" })     // false
println(Shape::Dot(1) == Shape::Dot(1))                       // true
println(Shape::Dot(1) == Shape::Box { side: 1 })              // false
```

Nesting composes, so an aggregate inside an aggregate is still compared by value:

```rust
struct Bag { items: Vec<int> }
println(Bag { items: vec![1, 2] } == Bag { items: vec![1, 2] })  // true
println(Some(vec![[1, 2]]) == Some(vec![[1, 2]]))                // true
println([vec![1]] != [vec![2]])                                  // true
```

A `Result` written with a bare `Ok` or `Err` still needs its type to be known, so
annotate the binding when nothing else pins the error type:

```rust
let a: Result<int, Error> = Ok(1)
let b: Result<int, Error> = Ok(1)
println(a == b)   // true
```

### Logical

| Operator | Description |
|----------|-------------|
| `and` | Logical AND (short-circuit) |
| `or` | Logical OR (short-circuit) |
| `not` | Logical NOT |

Short-circuit evaluation: `a and b` doesn't evaluate `b` if `a` is false. Same for `or` with true.

### Bitwise

| Operator | Description |
|----------|-------------|
| `&` | Bitwise AND |
| `\|` | Bitwise OR |
| `^` | Bitwise XOR |
| `~` | Bitwise NOT |
| `<<` | Left shift |
| `>>` | Right shift (arithmetic on `int` and signed types, logical on unsigned types) |

Both operands have the same integer type. On `int` the shift count is taken modulo 64 and
`<<` traps when the exact result leaves the 48-bit range, like `*`. On a sized type the
count is taken modulo the width and the result wraps.

### Precedence (lowest to highest)

1. `or`
2. `and`
3. `not`
4. `==`, `!=`, `<`, `<=`, `>`, `>=`
5. `|`
6. `^`
7. `&`
8. `<<`, `>>`
9. `+`, `-`
10. `*`, `/`, `%`
11. Unary `-`, `~`, `not`
12. Call `()`, value member access `.`, module and associated paths `::`

When in doubt, use parentheses

## Control Flow

### Condition position

The headers of `if`, `while`, `for`, and `match` are parsed in condition
position, where a `{` that follows a path always opens the block rather than
starting a struct or enum construction. This keeps `if flag { ... }` unambiguous
without requiring parentheses around ordinary conditions, and it means a
construction written directly in a header is a syntax error:

```rust
struct Point { x: int }
if Point { x: 1 }.x > 0 { println("yes") }   // ✗ error: expected semicolon or newline, found :
```

Parenthesise the construction to build one there:

```rust
struct Point { x: int }
if (Point { x: 1 }).x > 0 { println("yes") }
```

The same applies to the other three headers:

```rust
struct Flag { on: bool }
struct Bag { items: Vec<int> }
struct Point { x: int }

let mut n = 0
while (Flag { on: n < 2 }).on { n++ }

for item in (Bag { items: vec![1, 2] }).items { println(item) }

println(match (Point { x: 1 }) { Point { x } => x })
```

### if/else

```rust
let n = 5

if n < 0 {
    println("negative")
} else if n == 0 {
    println("zero")
} else {
    println("positive")
}
```

Braces are required. Parentheses around conditions are not.

`if` is also an expression. Every branch is a block whose value is its last
statement when that statement is an expression (with or without a trailing `;`),
an `if` that has an `else`, or a nested block; any other last statement, and an
empty block, make the branch `unit`. A chain of any length yields the value of
the branch it takes, in every position: a function or closure tail, a `let`
initialiser, a match arm, an argument, a `return` operand, inside another block.

```rust
fn grade(score: int) -> int {
    if score >= 90 { 4 } else if score >= 80 { 3 } else if score >= 70 { 2 } else { 0 }
}

let bonus = if grade(85) == 3 {
    let extra = 1
    10 + extra
} else if grade(85) == 4 { 20 } else { 0 }
println(bonus)   // 11
```

All branches must have the same type. In expression position an `if` needs an
`else` (E0101 `expected else`); as the tail of a function whose return type is not
`unit`, an `if` without `else` can fall through and is E0215 `function can fall
through without returning int`, never a silent `unit`. A function or closure
without a return type whose tail is not a value (an `if` without `else`, a `let`,
a loop, an assignment) returns `unit`, so `let x: int = f(1)` is E0301; if such
a body also returns a value with `return`, it can fall through and is E0215. An
assignment `x = v` or `x += v` is never the value of such a tail, so
`fn set(c: bool) { if c { count = 1 } else { name = "b" } }` is a `unit` function.
Branches that are both values must still agree: `if c { v.push(1) } else { v.pop() }`
as the tail of a function without a return type is E0301.

### Block expressions

A `{ ... }` in expression position is a block expression with its own scope. Its
value follows the same rule as an `if` branch.

```rust
let x = 1
let area = {
    let x = 10
    let width = x * 2
    width * x
}
println(area + x)   // 201
```

### if let

`if let PATTERN = value { ... } else { ... }` runs the first block when the
pattern matches, with the pattern's bindings in scope only inside that block.
`else` is optional, and `else if` / `else if let` chain as usual, after an `if`
or an `if let`, in statement and expression position. It is an expression: when
the chain ends with an `else` block every block gives its value and they must
have the same type; a chain without a final `else` is `unit`, and is only
allowed in statement position.

```rust
let found: Option<int> = Some(7)
if let Some(v) = found { println(v) } else { println("none") }

let doubled = if let Some(v) = found { v * 2 } else { 0 }
println(doubled)   // 14
```

### while

```rust
let mut n = 0
while n < 3 {
    n++
}
println(n)   // 3
```

### while let

`while let PATTERN = value { ... }` evaluates `value` before every iteration and
stops at the first value the pattern does not match. `break` and `continue` act on
it like on any loop.

```rust
let mut stack = vec![1, 2, 3]
let mut sum = 0
while let Some(top) = stack.pop() {
    sum += top
}
println(sum)   // 6
```

### loop

`loop { ... }` repeats until a `break` or a `return`. It is an expression:
`break value` leaves the loop with that value, a bare `break` gives `unit`, and a
loop no `break` leaves has the type `never`, so it satisfies any return type.
Every `break` of one loop must carry values of the same type.

```rust
let mut i = 0
let first_square_above_50 = loop {
    i += 1
    if i * i > 50 { break i }
}
println(first_square_above_50)   // 8
```

`break value` is only allowed for `loop`; on a `while`, `while let` or `for` it is
E0391. The `break` values of one `loop` must share a type (E0301).

### for

Iterates over integer ranges:

```rust
let start = 0
let end = 6

for i in start..end {          // exclusive: start to end-1
    println(i)
}

for i in start..=end {         // inclusive: start to end
    println(i)
}

for i in start..end step 2 {   // with step
    println(i)
}
```

The loop variable is immutable within the body.

### for-each

Iterates over the elements of a collection:

```rust
for letter in "Keine" {
    println(letter)
}
```

Works with string variables too:

```rust
let name = "Mokou"
for c in name {
    println(c)
}
```

Arrays and vectors support for-each iteration as well:

```rust
let arr = [10, 20, 30]
for item in arr {
    println(item)
}
```

```rust
let v = vec!["alice", "bob", "charlie"]
for item in v {
    println(item)
}
```

String iteration is Unicode-aware: each `c` is a `char`, one Unicode scalar value, not a byte, so multi-byte characters like `é` or `😀` come out whole.

```rust
let mut count = 0
for c in "café" {
    count++
}
// count == 4 (not 5, because é is considered as one character)
```

You can use `break` and `continue` inside for-each loops:

```rust
for c in "abcdefgh" {
    if c == 'd' { break }
    println(c)    // prints a, b, c
}
```

### break and continue

```rust
for i in 0..100 {
    if i == 50 { break }      // exit loop
    if i % 2 == 0 { continue }  // skip to next iteration
    // ...
}
```

Work in `while`, `while let` and `loop` too.

### Labels

A loop may carry a label, `'name:`, written before `for`, `while` or `loop`.
`break 'name` and `continue 'name` act on that loop from any depth of nested
loops, and `break 'name value` gives a labeled `loop` its value. An unknown label
is E0390. A closure created inside a loop body keeps its own copy of the
variables of its iteration, the loop variable and the names declared in the body,
including when the iteration ends by `break` or `continue`; a variable declared
outside the loop stays shared, so every closure sees its later writes.

```rust
let mut pairs = 0
'rows: for x in 0..10 {
    for y in 0..10 {
        if y > x { continue 'rows }
        if x == 5 { break 'rows }
        pairs += 1
    }
}
println(pairs)   // 15

let hit = 'search: loop {
    for a in 1..20 {
        for b in a..20 {
            if a * b == 91 { break 'search a * 100 + b }
        }
    }
    break 0
}
println(hit)   // 713
```

## Tuples

A tuple groups 2 to 8 values of possibly different types. The type is written
`(A, B, ...)`, a value `(a, b, ...)`, and an element is read with `.0`, `.1`, ...
in declaration order. `(x)` is just a parenthesised `x`.

```rust
fn divmod(a: int, b: int) -> (int, int) { (a / b, a % b) }

let t = divmod(17, 5)
println(t.0)   // 3
println(t.1)   // 2

let pair: (int, string) = (1, "one")
let nested = ((1, 2), 3)
println(nested.0.1)   // 2
```

Tuples are immutable: writing an element, `t.0 = 1` or `t.0 += 1`, is E0393. They
are aggregate values shared by reference (like structs, see
[Structs](#structs)), which is never observable because no element can change.
`==` compares them element by element. Tuples can be returned, passed, stored in
`Vec`, arrays, struct fields and enum payloads, used in generic functions, and
returned from a module; the tuple fields and payloads of a type imported from
another module read and match like local ones. A tuple has no `Display`; print its elements. Reading an
element past the arity, `t.2` on a pair, is E0363 `tuple (int, int) has no field
2`. Names starting with `AelysTuple` are reserved for the compiler's tuple types:
writing one anywhere (a type, a struct, a variable) is E0394.

### Destructuring let

`let` accepts a tuple pattern or a struct pattern on its left side. The pattern
must be irrefutable: it may contain only names, `_`, tuple patterns and struct
patterns (`..` allowed). `let mut` makes every name it binds mutable. A literal,
range, variant or alternation is E0108 `refutable pattern in let`; use `match` or
`if let` for those. A type that does not fit the pattern (wrong tuple arity, wrong
struct) is E0301; a wrong arity says `tuple pattern has 3 elements, but the value
is a tuple of 2 elements`, in `let` and in `match`.

```rust
struct Point { x: int, y: int }

let (q, r) = divmod(23, 4)
let (_, middle, _) = (7, 8, 9)
let mut (a, b) = (1, 2)
a += 10

let Point { x, y } = Point { x: 3, y: 4 }
println(q + r + middle + a + b + x + y)
```

## Modules

### Imports

The `needs` keyword imports modules:

```rust
needs std::fs                     // whole module
needs std::math as m              // aliased
needs sqrt, pow from std::math    // multiple functions
```

Safe stdlib modules (io, math, string, convert, time) are auto-registered, so you can call their functions directly without `needs`. You can still use `needs` with them for aliasing or selective imports if you want:

```rust
needs std::math as m              // now use m::sqrt() instead of math::sqrt()
needs sqrt, pow from std::math    // import specific functions
```

With an alias (`needs std::math as m`), only the aliased form works: `m::sqrt()`.

After `needs sqrt from std::math`, you call `sqrt()` directly without the module prefix.

A whole-module import without an alias also binds the module's exports by their
bare names. Two such imports may export the same name: `needs std::fs` next to
`needs std::bytes` is accepted, and `fs::size(...)` or `bytes::size(...)` are
both fine. Only an unqualified use of the shared name is refused, at the use, with
E0445 `symbol 'size' is exported by multiple modules: bytes, fs`; assigning to
the bare name is the same E0445. A name imported
by name with `needs size from std::bytes` wins over the whole-module imports, and
two `needs size from ...` lines naming different modules are E0410 at the second
line, whose hint is to import the name from one module only.

Aliases and bare names belong to the file that writes them. `needs util as u` in
one module and `needs other as u` in another give each module its own `u`, and a
name a file imports never changes what another module's code reads.

A declaration of your own always takes precedence over an auto-registered name.
A program that defines `fn sqrt(t: int) -> int`, `fn min(...)`, `fn find(...)`
or `fn len(...)` calls its own function in every position: a direct call, an
argument, a collection element, a field initializer, a return value, a closure
body, a method body, a nested function and a function value, including a use
that comes before the declaration: every top-level `fn` and `impl` is defined
before the first statement of the script runs, so `let top = sqrt(4)` followed
by `fn sqrt` calls the user function. The qualified
`math::sqrt` and `math::min` still name the standard library functions. In the
same way, a method declared on a user type, directly or through a trait, takes
precedence over a built-in string or collection method of the same name:
`dir.find("a")`, `dir.len()` or `bag.push(x)` call the user methods.

### Standard Library Modules

The safe standard library modules are **auto-registered**, their functions are available without any `needs` statement, in the entry file and in every module it imports alike:

- `std::io` - console I/O
- `std::math` - math functions and constants
- `std::string` - string manipulation
- `std::convert` - type conversions
- `std::time` - time and timers


See [Standard Library](standard-library.md) for full documentation.

Native modules (shared libraries written against the aelys-native ABI, such as the SDL2 module in `modules/sdl`) are imported with `needs` like any module; how to write, build and load one, and the rules for byte buffers they borrow, are in [Native modules](native-modules.md).

### Custom Modules

Any `.aelys` file is a module. If you have:

```
project/
  main.aelys
  utils.aelys
  lib/
    helper.aelys
```

From `main.aelys`:
```rust
needs utils              // imports utils.aelys
needs lib::helper        // imports lib/helper.aelys
```

Top-level definitions in a file become the module's exports. A `pub const`
is exported with its value, so an importer can use it as a repeat count or an
array length (`[0; cfg::W]`); see [Constants](#constants).

A dotted path is resolved from the file that writes the `needs` line: `needs
lib::util` in `game/mobj.aelys` looks for `game/lib/util.aelys` (then
`game/lib/util/mod.aelys` and the native forms) first, and only then for
`lib/util.aelys` under the project root, the directory of the entry file. The
first file found is the module. Resolution depends only on the importing file and
the path, never on the order of `needs` lines nor on what another file imported
first.

A module is its file. One file reached by two spellings, `needs game::lib::util`
from the entry file and `needs lib::util` from `game/mobj.aelys`, is one module:
its top-level code runs once and its variables exist once. Two different files
that the same dotted path names from two different importers are two modules.

A top-level `let` is a variable of the module that declares it, and it has one
slot per module. Every reference inside that module resolves to that slot: the
module's own top-level code, its free functions, and the bodies of its impls,
whether such a body runs in the module or in a file that imported it. Two modules
that each declare `v` hold two variables, and a file that imports one of them and
also declares its own `v` holds a third, which shadows the import. An imported
variable, read bare or as `module::v`, reads the declaring module's slot, so it
always shows the module's current value. Assigning to a bare imported `pub let
mut` writes the module's variable; assigning to an imported `pub let` is E0446
`cannot assign to 'v': it is an immutable variable of the module that declares
it`. Assigning to an imported function name is E0446 too (`it is a function of
the module that declares it`), unless the file declares its own function of that
name, which shadows the import and keeps the local E0203. `module::v = ...` is
not an assignment target.

An imported variable's mutability carries to the places rooted at it. Through a
bare imported `pub let mut`, field, index and `push` writes (`ms.x = 3`,
`mh.p.x += 1`, `mv[0] = 10`, `mv.push(4)`) mutate the module's value, and the
module sees them. Through an imported `pub let` they are refused (E0330, E0318).
A struct, array or `Vec` is shared by reference, so binding one reached from an
imported `pub let` mutably, bare or qualified (`let mut w = v`,
`let mut w = cnt::v`, `let mut z = cnt::h.p`), is E0323. A
qualified place (`cnt::ms.x = 3`) is not writable; write through the bare import.

A module's functions, private and public, live in that module's own slots. A
private helper `helper` in one module and a same-named function of another module
or of the entry file are distinct functions: each module's code calls its own.

A generic declaration crosses the boundary as a template that every importer
specializes with its own types. A `pub fn` with type parameters is imported like
any other function, bare, qualified (`container::largest(v, key)`, `c::largest`)
or by name (`needs largest from container`), and each call instantiates it in the
importing file. A `pub struct` or `pub enum` with type parameters is imported the
same way, with its impls, and `Stack<int>` built in the module and `Stack<int>`
built in the importer are one type. The carried body keeps its module's scope:
it reads the module's private functions, variables and private generic
declarations, calls the module's other generic functions, and a bound such as
`T: Ranked` or `T: Display` is satisfied by the importer's impls. A bare generic
name exported by two whole-module imports is E0445 at the use, two selective
imports of it are E0410, and a file's own function or local of that name shadows
the import. A private generic function stays private: importing it by name is
E0405.

A struct or enum value keeps its type across modules at run time: a value built
in one module is read, matched and passed as the same type in every module that
uses it, nested in another struct, in a `Vec`, an array, an `Option` or an enum
payload. A runtime error raised inside a module's function reports that module's
file and line.

### Visibility

The `pub` keyword marks something as explicitly public:

```rust
pub fn api_function() {
    // ...
}

fn internal_function() {
    // ...
}
```

A method is reachable wherever its type is, so `pub` written on an item of an
`impl` block (`pub fn new(...)`) is accepted and changes nothing.

### Cross-module types

`pub` also applies to a `struct`, an `enum`, and a `trait`, and a public one can
be used from another module. A type is imported by name with the selective form
of `needs`, not reached through a module path: `module::Name` does not name a
type. Given `shapes.aelys`:

```rust
pub enum Kind { Round, Sharp }

pub struct Point { x: int, y: int }

pub trait Scorable { fn score(self) -> int; }

impl Scorable for Point { fn score(self) -> int { self.x + self.y } }
```

an importer writes:

```rust
needs Point, Scorable from shapes

println(Point { x: 1, y: 2 }.score())
```

The selective form is exactly selective. A public type the `from` list does not
name stays out of scope, and using one is E0378 `'Kind' is exported by module
'shapes' but this file does not import it`, which names the module and the
`needs` line that would fix it. The whole-module form `needs shapes` is
unchanged and still brings every public type into scope.

A public type that an imported function returns or takes is reachable without
being named: `needs spawn from game::mobj` lets `spawn(5).hp` read a `pub` field
of the returned `Mobj` and call `Mobj`'s inherent methods. Writing the name
`Mobj` itself, in a literal or an annotation, still needs `needs Mobj from
game::mobj` (E0378).

An impl written in the defining module travels with its type, but only when
every end of the impl is itself in scope. An inherent `impl Point` arrives with
`Point`. A trait impl `impl Scorable for Point` arrives only when the importer
names both `Point` and `Scorable`, which is why the example above imports the
trait as well; naming only `Point` leaves `score` unresolved, and naming only
`Scorable` does not put `Point` in scope. An enum and a trait import the same
way:

```rust
needs Kind from shapes

println(match Kind::Round { Kind::Round => 1, Kind::Sharp => 2 })
```

```rust
needs Scorable from shapes

struct Local { n: int }
impl Scorable for Local { fn score(self) -> int { self.n } }
println(Local { n: 5 }.score())
```

Importing a type that is not `pub` is E0403 `'Point' is not public in module
'shapes'`.

An impl body the importer inlines is still checked and compiled in its defining
module's terms. Everything that body reads travels with it: the types, traits and
free functions the defining module imported, the names it reaches through a module
path or through a module alias (a standard library alias such as `needs std::bytes
as by` included), the auto-registered modules, the module's own globals, public or
not, and the types its free functions return, and the
module's **own private `struct`, `enum` and `trait` declarations together with the
impls over them**, on the same gate as its private globals. The
importer never has to write those `needs` lines, and never sees the names they
bind: naming one itself is E0403 `'Hidden' is not public in module 'support'`,
which offers no `needs` line, there being none that would reach it. A diagnostic
about such a body reports against the file that owns the
statement, however many modules deep it lies, not against the importing file, and
a repair it offers is written in the terms of that file: an ambiguity raised
inside a carried body names a trait that file can write, never one the importing
file declared.

A private declaration that reaches the importer only inside a carried impl body
travels with that body and coexists with a same-named declaration of the
importing file: each resolves in its own module's terms. E0410 still applies
where the name participates in the import contract — an associated binding, a
signature, a nominal the importing file names in its own declarations where the
carried body can read it, or a trait together with its own declaration. A
**generic** private declaration travels as a template and monomorphizes in the
importer, whether the carried body names it or reaches it through the signature
of a private helper (`fn wrap(n: int) -> Cell<int>`). One that reaches the import
contract itself (an impl header, a trait) is E0407.

A file binds each nominal name once. Two declarations of one name reaching one
file is E0410 `symbol 'K' is exported by multiple modules: cored, corec`, reported
at the importer whether or not the file named either declaring module; when the
declaration only arrives with a body the file inlines, the report adds
`note: carried into this file by:` and the modules the file did write. The report
does not depend on the order of the `needs` lines. A selective import has no `as`
form, so the repair E0410 names is to import the name from one module only, or,
when the file declared the other itself, to rename its own declaration.

A public nominal type may cross a module boundary, including when some of its
fields are private. A private nominal type in a public signature is still rejected
at the point of declaration with E0407:

```rust
pub struct Point { pub x: int, y: int }

pub fn make() -> Point { Point { x: 1, y: 2 } }
// ✓ the public nominal type is exportable; `y` remains private

struct Hidden { value: int }
pub fn leak() -> Hidden { Hidden { value: 1 } }
// ✗ error[E0407]: 'Hidden' cannot be exported from module 'shapes'
```

Fields are private by default. A `pub` field of a public nominal type may be read,
written, used in a literal, or bound in a pattern from another module, subject to
the normal mutable-root rule for writes. A private field may be used only in its
defining module and descendant modules. An unrelated importer or sibling module
cannot read or write it; attempting either is E0412. Naming a private field in a
literal or pattern is E0413. A rest pattern may omit private fields without
revealing them, but it may not name them:

```rust
needs shapes
let point = shapes::make()
point.x                              // ✓ public field
point.y                              // ✗ error[E0412]
match point { Point { y, .. } => y } // ✗ error[E0413]
```

E0412 and E0413 include the source span, the nominal owner module, the current
module, the rejected operation, and a help message. The compiler carries the
stable nominal owner and field ordinals through imports, impl bodies, and patterns;
visibility is checked before typed AST emission, while lowering uses only the
validated fixed-layout schema ordinal.

The restriction is on the exported signature's nominal visibility, not on field
privacy. A private function in the defining module may take and return the type
freely, and a descendant module may use its private fields:

```rust
pub struct Point { pub x: int, y: int }
fn make() -> Point { Point { x: 1, y: 2 } }
pub fn depth() -> int { make().y }
```

## Function Attributes

### @inline and @inline_always (Function Inlining)

These attributes tell the compiler to substitute a function's body directly at the call site, avoiding function call overhead

```rust
@inline
fn add(a: int, b: int) -> int {
    a + b
}

@inline_always
fn clamp(x: int, min: int, max: int) -> int {
    if x < min { return min }
    if x > max { return max }
    x
}
```

The difference:

| Attribute | Behavior |
|-----------|----------|
| `@inline` | Hint to the compiler. Respects code size thresholds. |
| `@inline_always` | Forces inlining. Ignores size limits. |

`@inline` can be ignored in some cases (recursive functions, mutual recursion). `@inline_always` forces substitution no matter what (except truly impossible cases like recursion)

**When to use `@inline`:**

- Small utility functions called frequently
- Thin wrappers around simple operations
- Performance-critical functions in tight loops

```rust
@inline
fn square(x: int) -> int { x * x }

@inline
fn is_even(n: int) -> bool { n % 2 == 0 }
```

**When to use `@inline_always`:**

- When you know inlining helps despite the size
- Functions with captures you still want inlined
- Micro-benchmarks where every cycle counts

**Limitations:**

Some functions simply can't be inlined:

| Case | Why |
|------|-----|
| Recursive function | Inlining would expand forever |
| Mutual recursion | Cycle like A → B → A detected |
| Native function | No Aelys body to substitute |

```rust
// ✗ Won't work
@inline
fn factorial(n: int) -> int {
    if n <= 1 { return 1 }
    n * factorial(n - 1)  // recursive!
}
```

The compiler will warn you if you try

**Functions with captures:**

Functions that capture variables from their enclosing scope are handled differently:

- `@inline`: won't be inlined (you'll get a warning)
- `@inline_always`: forces it anyway

```rust
fn make_adder(x: int) {
    @inline_always
    fn add(y: int) -> int {
        x + y  // captures x
    }
    return add
}
```

Use `@inline_always` here only if you know what you're doing

**Public functions:**

Functions marked `pub` with `@inline` get inlined locally, but the original code is kept for callers from other modules:

```rust
@inline
pub fn helper() {
    // inlined within this module
    // still available for importers
}
```

**Automatic inlining:**

Even without any attribute, the optimizer already inlines:

- Trivial functions (3 statements or less)
- Functions called only once

So you don't need to annotate everything. `@inline` and `@inline_always` are for when you want explicit control !

**Optimization never changes what a program does:**

Every optimization level (`-O0` to `-O3`) produces the same output and exit code as `-O0`, and raises the same runtime error at the same point of the run. Only the location reported for an error raised inside an inlined function can differ: it is the line of the call that was inlined, and the stack trace has no frame for the inlined function. An inlined call evaluates each argument exactly once, left to right, before the body, as a real call does: an argument whose parameter is unused still runs, and one that traps (division by zero, an out-of-bounds index, an integer overflow, `<<` on `int` leaving the 48-bit range, a NaN or infinite float converted with `as`, a global read before its initialiser ran) traps at the same point. A call is only inlined when its name resolves to the top-level `fn`; a parameter, a local closure or a nested `fn` of the same name is called as written. An unused `let` is only removed when its initializer can neither write anything nor trap; `let _x = v[i]`, `let _x = a / b`, `let _x = a * b` or `let _x = g.x` with `g` a global bound after code ran still raise their error when they would at `-O0`. Operators on the sized types wrap and never trap, except `/` and `%` by zero.

## Semicolons

Optional. The parser automatically inserts them after certain tokens (like Go does):

```rust
let x = 1
let y = 2

// same as:
let x = 1;
let y = 2;
```

Explicit semicolons let you put multiple statements on one line:

```rust
let x = 1; let y = 2; let z = 3
```

A newline directly inside `(...)` or `[...]` inserts nothing, so arguments and
elements may span lines. A `{` opens a block where newlines end statements again,
even inside parentheses or brackets, so a multi-line block, `if`, `match` or
closure body works as an argument, a `vec!` element or a tuple element:

```rust
let total = apply(fn(a: int) -> int {
    let doubled = a * 2
    doubled + 1
})
```

## Collections

### Arrays and vectors

`[T; N]` and `Vec<T>` are distinct types. An array has a fixed length; a Vec
owns a growable sequence. The literal forms are deliberately Rust-like:

```rust
let points = [10, 20, 30]
let empty: [int; 0] = []
let values = vec![1, 2, 3]
let typed: Vec<int> = vec![]
```

`[value; N]` creates an array with a compile-time non-negative constant length.
`N` is an integer literal, an arithmetic expression over such literals, a
[constant](#constants) (`const N`, or another module's `cfg::N`), or an
associated constant named on a concrete type or on `Self`, the same lengths the
annotation `[T; N]` accepts. `vec![value; count]` creates a Vec and permits a
dynamic count. There is no runtime-sized array constructor; use a Vec when the
count is not known at compile time.

The element is evaluated once. A scalar or string element is stored in every
slot. An aggregate element (a struct, enum, `Option`/`Result`, array, `Vec` or
`Map`) is deep-copied into each slot, as by `clone()`, so every slot holds its own
object, including when the element is an existing binding:

```rust
let zeros = [0; 10]
let repeated = [7; 3]
let mut buffer = vec![0; 4]
let mut grid = [[0; 4]; 3]
grid[1][2] = 5                    // only row 1 changes
let mut mobjs = vec![Mobj { x: 0, y: 0 }; 128]
mobjs[0].x = 10                   // only slot 0 changes
```

All elements must have one common type. A fixed array annotation is checked at
compile time, including its length:

```rust
let values: [int; 3] = [1, 2, 3]
let mut numbers: Vec<int> = vec![1, 2]
numbers.push(3)
numbers[0] = 9
```

Mutation requires `let mut`, a mutable parameter, or a mutable root binding
for the place, however deep: `v[i] = x`, `w.items.push(x)` and `w.grid[i][j] = x`
all follow the root (see [Structs](#structs)). Collections are reference
types: `let mut alias = values` shares the Vec when `values` is mutable and is
E0323 when it is not; `values.clone()` copies it. `push`, `pop`, and `reserve` are Vec operations. `pop`
and `get` return `Option<T>` and therefore must be handled as values:

```rust
let mut values = vec![10]
let item = values.get(0)
let answer = match item {
    Some(value) => value,
    None => 0,
}
```

Indexing with a constant index that is provably outside a known collection
length is a compile error. A length is known for an array or Vec literal indexed
directly, for a fixed-size array type `[T; N]`, and for an immutable array
binding. A Vec binding never has a known length, even when it is immutable,
because `push` and any `mut` parameter it is handed can grow it; neither does a
`let mut` array of unsized type, which another function or a closure can
replace. Those indices are checked at run time. Other invalid indices trap at runtime; they never
produce a null value. `len` and `is_empty` are available on arrays and Vecs.

For-each accepts an array, Vec, or string. `for item in &values` makes the
receiver read-only for the loop body; `for item in values` has the same
read-only borrow while the loop is executing. A bare `iter()` result is not a
surface value and must be consumed by a pipeline:

```rust
let doubled = vec![1, 2, 3].
    iter().
    map(fn(value: int) -> int { return value * 2 }).
    collect()

let even = doubled.filter(fn(value: int) -> bool { return value % 2 == 0 })
let total = even.fold(0, fn(sum: int, value: int) -> int { return sum + value })
```

A chain broken across lines keeps the `.` at the end of the line it continues
from. Semicolon insertion ends a statement at the newline, so a line that begins
with `.` is a fresh statement and does not parse. A chain written on one line
needs no such care.

`map` and `filter` return owned Vecs. `fold` returns its accumulator.
`collect` copies its input into an owned Vec. Slices are also owned Vecs:

```rust
let source = [1, 2, 3, 4]
let middle = source[1..3]
let inclusive = source[1..=2]
```

Slice bounds are checked statically when all operands and the source length are
known, and otherwise trap on an invalid runtime range.

Ranges are values as well as slice syntax. Open and closed forms are available:
`..end`, `start..`, and `start..=end`. A range value can be bound and reused for
a slice, for example `let span = 1..3; source[span]`. A range without a valid
collection context is still a type error.

### Vec and array methods

Beyond `len`, `is_empty`, `get`, `push`, `pop`, `capacity` and `reserve`, a Vec
has these methods. Every method that changes the collection needs a mutable
receiver (E0318); every index is checked at run time and an invalid one traps
with `index out of bounds: index I is out of bounds for length N`.

| Method | Result | Meaning |
|--------|--------|---------|
| `v.insert(i, x)` | `unit` | insert before index `i`; `i == len` appends |
| `v.remove(i)` | `T` | remove and return the element at `i`, shifting the rest left |
| `v.swap_remove(i)` | `T` | remove the element at `i` and move the last element into its slot |
| `v.clear()` | `unit` | remove every element |
| `v.extend(w)` | `unit` | append a copy of every element of the Vec or array `w` (`&w` is accepted) |
| `v.append(w)` | `unit` | append every element of the Vec `w`, then empty `w` (which must be mutable) |
| `v.truncate(n)` | `unit` | keep the first `n` elements; a larger `n` does nothing |
| `v.resize(n, x)` | `unit` | truncate to `n`, or grow to `n` with copies of `x` |
| `v.swap(i, j)` | `unit` | exchange two elements |
| `v.reverse()` | `unit` | reverse in place |
| `v.contains(x)` | `bool` | whether some element is `==` to `x` (structural) |
| `v.index_of(x)` | `Option<int>` | index of the first element `==` to `x` |
| `v.sort()` | `unit` | stable ascending sort |
| `v.sort_by(f)` | `unit` | stable sort with `f: fn(T, T) -> int`, negative when the first argument goes first |
| `v.first()`, `v.last()` | `Option<T>` | the first or last element |
| `v.set(i, x)` | `unit` | `v[i] = x` as a method |
| `v.fill(x)` | `unit` | overwrite every element with `x` |
| `v.join(sep)` | `string` | concatenate a `Vec<string>` with `sep` between elements |

A fixed array `[T; N]` has the methods that keep its length: `swap`, `reverse`,
`contains`, `index_of`, `sort`, `sort_by`, `first`, `last`, `set` and `fill`.
`insert`, `remove`, `swap_remove`, `clear`, `extend`, `append`, `truncate` and
`resize` on an array are E0311.

`sort` orders integers and floats numerically (NaN after every number), strings
lexicographically by scalar value, chars by scalar value, and `false` before
`true`; any other element type is E0471, which names `sort_by` as the way out.
`sort_by` is a merge sort: it calls the comparator O(n log n) times, and a
comparator that changes the length of the Vec it is sorting traps with
`vector changed length while sort_by was running its comparator`.

`resize` and `fill` store one value in many slots. Like a repeat literal, an
aggregate value (a struct, enum, `Option`/`Result`, array, `Vec` or `Map`) is
deep-copied into each slot, as by `clone()`, so every slot holds its own
object. Scalars, strings, chars and closures are stored as they are.

Inside a generic function the element type may be a type parameter. E0471 is
then checked on every instantiation, and reported at the `sort` call when one
instantiation's element type breaks the rule.

### Maps

`Map<K, V>` is a built-in hash map with reference semantics, like `Vec`. The key
type is `int`, a sized integer, `string`, `char` or `bool`; any other key type,
written or inferred, is E0470. A generic `Map<K, V>` is checked on every
instantiation of the generic code, so `mk(1.5, 2)` for a generic `mk` is E0470
at compile time. A Map that would grow past `-ae.max-heap` fails with
`out of memory` at the insert.

```rust
let mut ages: Map<string, int> = Map::new()
ages["reimu"] = 17
ages["marisa"] = 16
ages["reimu"] += 1
let seen = ages.contains_key("sakuya")      // false
let marisa = ages.get("marisa")             // Some(16)
let _ = ages.insert("youmu", 15)            // None: the key was new
for name in ages.keys() { println(name) }   // reimu, marisa, youmu
```

| Operation | Result | Meaning |
|-----------|--------|---------|
| `Map::new()` | `Map<K, V>` | an empty map; `K` and `V` are inferred |
| `m.insert(k, v)` | `Option<V>` | store `v` under `k`, returning the previous value |
| `m.get(k)` | `Option<V>` | the value under `k` |
| `m.contains_key(k)` | `bool` | whether `k` is present |
| `m.remove(k)` | `Option<V>` | remove `k`, returning its value |
| `m.len()`, `m.is_empty()` | `int`, `bool` | entry count |
| `m.clear()` | `unit` | remove every entry |
| `m.keys()`, `m.values()` | `Vec<K>`, `Vec<V>` | fresh Vecs of the keys or values |
| `m[k]` | `V` | the value under `k`; a missing key traps with `map key not found: k` |
| `m[k] = v` | `unit` | insert or overwrite; `m[k] += x` reads then writes |

`insert` and `remove` return an `Option`, which must be used like any other
(`let _ = m.remove(k)`); `m[k] = v` is the statement form of an insert. Mutation
needs a mutable receiver (E0318). Iteration order is insertion order: a new key
goes last, overwriting a key keeps its place, and removing a key does not
reorder the others. A map is not iterable directly (E0365 points at `keys()` and
`values()`). `==` on two maps is structural and ignores order: same key set and
`==` values. A map whose key and value types are both scalars, `char` or
`string` implements `Display`, rendering `{k: v, k: v}` in insertion order.
Lookups, inserts and removals are O(1) on average.

### Characters

`char` is one Unicode scalar value, stored unboxed. `c as int` gives its scalar
value, and `c as u8` (or any sized integer type) keeps the low bits of that value
(`'é' as u8` is `233`, `'€' as u8` is `172`); `n as char` converts any integer and traps with `invalid char: N is not a
Unicode scalar value` outside `0..=0x10FFFF` or inside the surrogate range.
`char` values compare with `==`, `!=` and the ordering operators, appear in
`match` patterns (`'a' | 'e' => ...`), and serve as Map keys. Methods:
`is_digit()` (ASCII `0`-`9`), `is_alpha()`, `is_alphanumeric()`,
`is_whitespace()` (Unicode properties), and `to_upper()`, `to_lower()`, which
return the character unchanged when its case mapping is not a single character
(`'ß'.to_upper()` is `'ß'`). `string::from_char(c)` makes a one-character
string, and `s.push(c)` appends one to a string variable or to a string place
reached from a mutable root (`p.name.push(c)`, `v[i].push(c)`, `m[k].push(c)`),
evaluating each index of the place once; on an immutable root it is E0318.

### String Indexing

Strings support character-based indexing with `[]`:

```rust
let s = "hello"
s[0]    // 'h'
s[1]    // 'e'
s[4]    // 'o'
```

Each index returns a `char`, the same type a for-each over the string yields.
Indexing is Unicode-aware: it accesses the Nth character, not the Nth byte, so it
walks the string from the start.

You can combine string indexing with a range-based for loop:

```rust
let s = "hello"
for i in 0..s.char_len() {
    println(s[i])
}
```

Or use for-each iteration for simpler code (see [for-each](#for-each)):

```rust
for c in "hello" {
    println(c)
}
```

Iterator pipelines and owned slices are part of the delivered collection model.
`iter()` is compiler-lowered and must be consumed by `map`, `filter`, `fold`, or
`collect`; a bare iterator is a compile-time error.

## Data-carrying enums

Enums are closed nominal sums. Variants may be unit, tuple, or named-field
constructors:

```rust
enum BulletKind {
    Rice,
    Spiral(int, float),
    Directed { speed: float, damage: int },
}

let kind = BulletKind::Directed { damage: 2, speed: 1.5 }
```

A `;` separates variants as well as a `,`, so the same declaration may be written
on one line:

```rust
enum BulletKind { Rice; Spiral(int, float); Directed { speed: float, damage: int } }
```

Constructors and patterns use `::`; `.` remains value member selection. Every
payload field occupies one immutable value slot. The compiler assigns the
constructor and variant IDs, checks their field count and types, and resolves
pattern field offsets before emitting bytecode.

Matching is exhaustive at compile time. The check is a usefulness analysis over
the whole pattern, so it looks through every level of nesting: `Ok(_)`,
`Err(AppE::Io(_))` and `Err(AppE::Bad(_))` together cover a
`Result<int, AppE>`, and `Some(K::A)`, `Some(K::B)` and `None` cover an
`Option<K>`, with no `_` needed. Guards do not cover a constructor. A missing
arm emits E0302 `non-exhaustive match; missing ...`, naming every missing
top-level constructor and, inside one, the precise nested shape that is left:
`missing C::Red, C::Blue`, `missing Err(AppE::Bad(..))`,
`missing Ok(Some(Mid::M(In::Y)))`, `missing E::A(false, false)`, with the help
line `add a missing arm or '_'`. Alternation may combine constructors when all
alternatives bind the same names and types.

Integer matches are checked over the value range of the scrutinee's type
(`int`, `i8` to `u32`): they need `_` unless their literals and
[range patterns](#range-patterns) cover the whole range, and the message names
the first uncovered interval, for example `missing 128` or `missing ..=-1`.
String matches always need `_` (or a binding).

A bare name in a pattern binds a new variable and matches everything. When the
value at that position is an enum (including `Option` and `Result`) and the name
starts with an uppercase letter, the pattern is almost certainly a misspelt or
unqualified variant, so it is E0392 `pattern 'Blu' is not a variant`, with
`did you mean Color::Blue?` when a variant is close. Write the variant with its
enum path, or bind with a lowercase name.

```rust
enum Color { Red, Green, Blue }
fn f(c: Color) -> int { match c { Color::Red => 1, Color::Green => 2, Blu => 3 } }
// ✗ error[E0392]: pattern `Blu` is not a variant: a bare name binds a new variable that matches every value
//   = help: did you mean Color::Blue?
```

```rust
enum BulletKind {
    Rice,
    Spiral(int, float),
    Directed { speed: float, damage: int },
}

fn score(kind: BulletKind) -> int {
    match kind {
        BulletKind::Rice => 1,
        BulletKind::Spiral(count, _) if count > 0 => count,
        BulletKind::Spiral(_, _) => 0,
        BulletKind::Directed { damage, .. } => damage,
    }
}

println(score(BulletKind::Directed { damage: 2, speed: 1.5 }))
```

A `match` used as a statement (not as a value and not as the last statement of a
block that gives a value) discards its arm values, so its arms need not agree:
`match o { Ok(v) => total = v, Err(_) => {} }` and arms that all `return` are
accepted. A discarded `Result` or `Option` arm value is still E0303/E0304. The
last statement of a script is its value, so a `match` there keeps its arm values.
An assignment (`r = Some(3)`) is never a discarded value, whatever its type.

A bare name in a pattern that names a `const` in scope matches the constant's value, as
a literal would; it does not bind a new variable, and E0392 never applies to it. A local
variable or parameter of the same name hides the constant, and then the name binds. A
`float` constant cannot be a pattern (E0301).

```rust
const LIMIT = 10
fn at_limit(x: int) -> int { match x { LIMIT => 1, _ => 0 } }
fn hit(x: int) -> int { if let LIMIT = x { 1 } else { 0 } }
println(at_limit(10) + hit(10) + at_limit(3))   // 2
```

### Unreachable patterns

Exhaustivity has a mirror rule. An arm that no value can reach is E0356
`unreachable pattern: E::A is already covered by an earlier arm`, with the help
line `remove the arm or move it above the arm that covers it`. Two shapes trigger
it: an arm placed after an unguarded wildcard, and a constructor repeated after an
earlier unguarded arm for the same constructor.

```rust
enum Kind { A, B }
fn after_wildcard(k: Kind) -> int { match k { _ => 0, Kind::A => 1 } }
// ✗ error[E0356]: unreachable pattern: Kind::A is already covered by an earlier arm

fn duplicate(k: Kind) -> int { match k { Kind::A => 1, Kind::A => 2, Kind::B => 3 } }
// ✗ error[E0356]: unreachable pattern: Kind::A is already covered by an earlier arm
```

An arm is also unreachable when the earlier unguarded arms together already
cover it, for example `Some(_)` after `Some(K::A)` and `Some(K::B)`, or `3..=5`
after `0..=9`.

A guard makes the earlier arm conditional, so a later arm for the same
constructor is still reachable and is accepted:

```rust
enum Kind { A(int), B }

fn classify(k: Kind) -> int {
    match k {
        Kind::A(value) if value > 0 => 1,
        Kind::A(_) => 2,
        Kind::B => 3,
    }
}

println(classify(Kind::A(5)))    // 1
println(classify(Kind::A(-5)))   // 2
```

### Range patterns

An integer pattern may be a range: `lo..=hi` (inclusive), `lo..hi` (end
excluded), `..=hi` and `lo..` (open). Bounds are integer literals, possibly
negative. A range applies to `int` and to the sized integers, whose bounds must
fit the type: a bound outside it is E0301 `integer literal 256 does not fit in u8`, and on
any other type a range is E0301. Exhaustiveness covers a sized integer by its own bounds
(`u8` is covered by `0..=255`, `i8` by `-128..=127`), which is sound because sized
arithmetic wraps, so a sized value never leaves its range. `int` is covered only by ranges
reaching its 48-bit bounds or by `_`. A `char` pattern (`'a'`) matches one character; the
`char` domain is open, so a `match` on a `char` needs a `_` arm. A range that matches nothing
(`9..=1`) or an inclusive range with no end is E0108. Ranges combine with `|`
and nest inside variants and tuples.

```rust
fn class(n: int) -> int {
    match n {
        ..=-1 => 0,
        0 => 1,
        1..=9 => 2,
        10.. => 3,
    }
}

fn half(b: u8) -> int { match b { 0..=127 => 0, 128..=255 => 1 } }
println(class(42) + half(200))   // 4
```

Enum values are heap objects traced through every payload slot. A malformed
bytecode object, schema ID, variant ID, field offset, or slot count is rejected
by the verifier or VM boundary; it cannot turn into null.

## Structs

Structs are nominal, fixed-layout records. The declaration, construction, field
read, and field mutation forms are:

```rust
struct Bullet { x: float, y: float, kind: string }

fn move_bullet() -> float {
    let mut bullet = Bullet { x: 0.0, y: 0.0, kind: "rice" }
    bullet.x = bullet.x + 1.0
    bullet.x
}
```

Struct names start with an uppercase letter. Fields are checked against their
declared types at compile time. A field place is resolved to its declared
offset, not looked up by a runtime string key.

### Reference semantics

Structs, enums, arrays and `Vec` are reference types, like classes in C# or
Java: a binding, a parameter, a field and a collection slot hold a reference to
one shared object. `let b = a`, passing `a` to a function, `v.push(a)`,
`S { inner: a }` and `Some(a)` do not copy `a`; they share it. Strings,
closures and scalars are values or immutable, so sharing them is unobservable.
A value returned from a function is treated as a fresh binding.

`x.clone()` makes a deep copy of a struct, enum, `Option`/`Result`, array,
`Vec` or `Map`: every nested struct, enum payload, array, `Vec` and `Map` value
is copied too, while strings, closures and `Map` keys are shared. A structure with cycles or shared children is
copied with the same shape, and a cycle does not make `clone` loop. A user
method named `clone` takes precedence over the built-in one.

```rust
struct P { x: int, y: int }

let mut a = P { x: 1, y: 2 }
let mut b = a          // b and a are the same object
b.x = 5                // a.x == 5
let mut c = a.clone()  // an independent copy
c.x = 9                // a.x is still 5
```

### Mutability is a property of the root binding

A place is a binding followed by any chain of fields and indexes: `p`, `p.x`,
`v[i]`, `w.ps[i].x`. A place is mutable when its root binding is: a `let mut`
binding, a `mut` parameter, or `self` in a `mut self` method. Every write form
works through any such place:

```rust
struct World { ps: Vec<P>, ids: [int; 4] }

let mut w = World { ps: vec![P { x: 0, y: 0 }], ids: [0; 4] }
w.ps[0].x = 10        // field write through an index
w.ps[0].x += 1        // compound assignment
w.ps.push(P { x: 2, y: 2 })
w.ids[3] = 7          // index write through a field
w.ps[1].shift(5)      // a `mut self` method on an element
```

`self.items.push(v)` and `self.items[i] = v` work in a `mut self` method,
including in an impl of a generic struct. Writing through an immutable root is
an error: a field write is E0330, an index write or `push`/`pop`/`reserve` is
E0318, and calling a `mut self` method is E0331. A temporary, such as the
result of a call, cannot be the root of a write.

Because aggregates are shared, creating a mutable alias of an aggregate that
is reachable from an immutable binding would let the immutable binding change.
It is E0323, at every place where an alias is created:

```rust
let a = P { x: 1, y: 2 }
let mut b = a                  // ✗ error[E0323]: cannot bind `a` mutably
let mut c = a.clone()          // ✓
let h = World { ps: vec![], ids: [0; 4] }
let mut ps = h.ps              // ✗ error[E0323]
let mut b = if c { a } else { P { x: 0, y: 0 } }   // ✗ error[E0323]
let mut d = P { x: 0, y: 0 }
d = a                          // ✗ error[E0323]: cannot assign `a` to the mutable binding `d`
fn bump(mut p: P) { p.x += 1 }
bump(a)                        // ✗ error[E0323]: cannot pass `a` to the `mut` parameter `p`
```

The message names the place and its root and suggests `a.clone()` or `let mut
a`. When the root is mutable, `let mut b = a`, `d = a` and passing `a` to a
`mut` parameter are allowed and share the object. Storing an aggregate in a
container or a field (`v.push(a)`, `S { inner: a }`) always shares it and is
never an error. A function that receives an immutable parameter can still
return it, and the caller may bind the result mutably: a returned value is a
fresh binding, and this route is not tracked.
A call through a function value applies the same rule at the `mut` positions
of the function's type (`fn(mut P) -> R`), see
[Mutable Parameters](#mutable-parameters).

The iteration variable of `for p in ps` and the bindings of a `match` pattern
are views of the iterated or matched place: `let mut q = p` inside the loop is
allowed when the root of `ps` is mutable, and shares the element; when the root
is immutable it is E0323 and the message names that root. Writing through `p`
itself (`p.x = ...`) is E0330 because `p` is an immutable binding; write
`ps[i].x = ...` with an index loop, or clone.

Methods and associated functions live in an `impl` block:

```rust
struct Point { x: int, y: int }

impl Point {
    fn origin() -> Point { Point { x: 0, y: 0 } }

    fn shift(mut self, dx: int) -> int {
        self.x = self.x + dx
        self.x
    }
}

let mut point = Point::origin()
point.shift(3)
```

An associated function has no `self` parameter and is reached with `Type::name`.
A method has `self` or `mut self` as its first parameter and is reached with
`value.name(...)`. Struct field access remains `value.field`; module members
and associated items use `::`.

Inside an impl body, `Self::name(...)` calls the associated function that
`Type::name(...)` calls, and inside a trait default body it calls the one of the
impl the body is serving. A method that takes `self` is not reachable through
either receiver: `Self::shift(point)` and `Point::shift(point)` are both E0362.

Struct patterns bind fields and compose with the exhaustive `match` rules:

```rust
struct Point { x: int, y: int }

let point = Point { x: 3, y: 4 }

println(match point {
    Point { x, y } => x + y,
})
```

An irrefutable field pattern covers the struct on its own, so adding `_` after one
is E0356; see [Unreachable patterns](#unreachable-patterns).

`Point { x: 0, .. }` is refutable and does not cover the remaining values. A
struct-only match must contain an irrefutable field pattern or `_`; otherwise
the compiler emits E0324: `non-exhaustive struct match for Point; add '_' or
an irrefutable field pattern`. Unknown or duplicate fields are compile errors.

## Generics

Functions, structs, and enums may declare type parameters. Calls infer concrete
arguments from their values or select them with turbofish syntax:

```rust
fn identity<T>(value: T) -> T { value }
let number = identity::<int>(7)

struct Box<T> { value: T }
let boxed: Box<int> = Box { value: 7 }
```

Generic functions are monomorphized at compile time. Every reachable instance
has a concrete signature and body; open type parameters are never erased to
`Any` in bytecode. A generic function item cannot be used as a value without a
concrete instantiation and is rejected with E0343 `cannot infer the concrete type
for generic parameter 'T'; add a type argument`. Recursive or excessive
instantiation is a named compile-time error: E0344 for a recursive instantiation
without a decreasing type argument, E0345 when the instantiation limit is
exceeded. The same bounded worklist applies to reachable generic `impl` method
instances; recursive type growth is rejected before code generation rather than
being allowed to exhaust the compiler.

Generic structs and enums use the same concrete instance rule. Their field and
variant descriptors are emitted with concrete types, and two different type
arguments produce two different checked schemas. Turbofish selects the arguments
on an enum path and on a struct construction as well as on a call:

```rust
enum Holder<T> { Full(T), Empty }
struct Crate<T> { value: T }

let held = Holder::<int>::Full(3)
let crated = Crate::<int> { value: 4 }

println(crated.value)
println(match held { Holder::Full(v) => v, Holder::Empty => 0 })
```

## Traits

Traits define statically selected methods, including default methods:

```rust
trait Scorable {
    fn score(self) -> int;
}

struct Point { x: int }
impl Scorable for Point {
    fn score(self) -> int { self.x }
}

Point { x: 7 }.score()
```

A trait method declared without a body is required of every impl. A trait method
declared with a body is a default that an impl may leave alone:

```rust
trait Greet {
    fn name(self) -> string;
    fn greet(self) -> string { "hi {self.name()}" }
}

struct Shrine { n: string }
impl Greet for Shrine { fn name(self) -> string { self.n } }

println(Shrine { n: "Reimu" }.greet())
```

An associated function is selected with `Trait::name` or `Type::name`; a value
method uses `value.name(...)`. Selection is resolved to one direct symbol at
compile time. Missing required methods, ambiguous methods, orphan impls, and
overlapping impls are compile errors. A trait may not share its name with a
struct or an enum, whatever the declaration order: that is E0326, reported on the
trait, so the left side of a `::` path names exactly one declaration.

### Bounds

A type parameter carries its bounds inline, and `+` joins several. Bounds on
generic functions are checked at each concrete call site; an unsatisfied bound is
E0338 `trait 'Scorable' is not implemented for int; add an impl or change the
bound`.

```rust
trait Scorable { fn score(self) -> int; }
trait Weighted { fn weight(self) -> int; }

struct Bullet { n: int }
impl Scorable for Bullet { fn score(self) -> int { self.n } }
impl Weighted for Bullet { fn weight(self) -> int { self.n * 2 } }

fn rank<T: Scorable + Weighted>(value: T) -> int { value.score() + value.weight() }

println(rank(Bullet { n: 3 }))
```

A `where` clause states the same bounds after the signature, which reads better
once several parameters are bounded:

```rust
trait Scorable { fn score(self) -> int; }
trait Weighted { fn weight(self) -> int; }

struct Bullet { n: int }
impl Scorable for Bullet { fn score(self) -> int { self.n } }
impl Weighted for Bullet { fn weight(self) -> int { self.n * 2 } }

fn rank<T, U>(a: T, b: U) -> int where T: Scorable, U: Weighted {
    a.score() + b.weight()
}

println(rank(Bullet { n: 3 }, Bullet { n: 4 }))
```

`where` is contextual, so it is still usable as an ordinary identifier.

### Supertraits

A trait may require another trait of its implementors with `trait A: B`. Inside a
default or an impl of `A`, the methods of `B` are available on `self`, and a bound
on `A` also satisfies a use of `B`:

```rust
trait Base { fn base(self) -> int; }
trait Extra: Base { fn extra(self) -> int; }

struct Bullet { n: int }
impl Base for Bullet { fn base(self) -> int { self.n } }
impl Extra for Bullet { fn extra(self) -> int { self.base() + 1 } }

fn run<T: Extra>(value: T) -> int { value.extra() }

println(run(Bullet { n: 7 }))
```

Implementing a trait requires implementing each of its supertraits for the same
type: an impl header that promises one without it is E0338. The requirement
applies to every impl in turn, so a chain of any depth obliges every link. A
diamond obliges the shared root once, not once per path.

A supertrait also carries its associated types and constants into the
inheriting trait; see [Inherited associated items](#inherited-associated-items).

### Display

`Display` is a compiler-known trait that every module sees without importing it.
It declares one method:

```
fn to_display(self) -> string
```

The built-in scalars satisfy it by a compiler rule rather than by a registered
impl: the signed and unsigned integer types, the floating point types, `bool`,
and `string`. For every other type you write the impl yourself, and the impl is
an ordinary coherent trait impl held to the same rules as any other.

```rust
struct Point { x: int, y: int }

impl Display for Point {
    fn to_display(self) -> string { "({self.x}, {self.y})" }
}

let p = Point { x: 1, y: 2 }
println(p)                        // (1, 2)
println(convert::to_string(p))    // (1, 2)
println(__tostring(p))            // (1, 2)
println("point is {p}")           // point is (1, 2)
println(p.to_display())           // (1, 2)
```

An enum is the same:

```rust
enum Color { Red, Blue }

impl Display for Color {
    fn to_display(self) -> string { match self { Color::Red => "red", Color::Blue => "blue" } }
}

println(Color::Red)      // red
println("{Color::Blue}") // blue
```

Rendering a struct or an enum that has no `Display` impl is E0338 `trait
'Display' is not implemented for Point; add an impl or change the bound`. Every
path that renders the value reports it: the direct argument of `println`,
`convert::to_string` and `__tostring`, the argument behind a `{}` placeholder,
and an interpolated `{p}` inside a format string.

The built-in aggregates are not nominal types and cannot carry an impl, so a
format string still renders an array, an `Option` or a `Result` structurally:

```rust
let arr = [1, 2, 3]
println("arr = {arr}")   // arr = [1, 2, 3]
```

## Associated Types and Constants

A trait may declare items other than methods: an associated **type**, which each
impl fills in with a type, and an associated **constant**, which each impl fills
in with a value. They live in two separate namespaces, so one trait may declare
`type X` and `const X: int` and both resolve, the position deciding which is
meant. A name in either namespace is read back with a projection, `Type::Name`.

### Declaring an associated item

An associated type declaration carries a name and nothing else. An associated
constant declaration carries a name and a type annotation, and no value:

```rust
trait Source {
    type Item
    const LIMIT: int
    fn next(self) -> Self::Item;
}
```

The asymmetry is load-bearing. A trait states no type for `type Item`, so there
is nothing for an impl's `type Item = int` to disagree with; a bound would be
such a statement and there is no syntax for one, `type Item: Display` being
E0101 `expected fn, type, or const in trait declaration, found :`. A default is
equally absent: `type Item = int` in a trait body is E0101 on the `=`. An
associated constant is the other way round, and its annotation is required:
`const LIMIT` with no `: int` is E0101 `expected :`.

The declaring trait may use its own items in the signatures and default bodies
below them, through `Self::`.

### Defining an associated item

An impl of the trait defines each item once, repeating the constant's declared
type and supplying a value:

```rust
struct Counter { value: int }

impl Source for Counter {
    type Item = int
    const LIMIT: int = 10
    fn next(self) -> int { self.value }
}
```

An impl must define **every** required item **exactly once**. Omitting one is
E0421, which names the item and offers the spelling that would define it, a
spelling the impl body accepts as written: `type Item = <type>` for an
associated type and `const LIMIT: int = <value>` for an associated constant,
the constant repeating the type the trait declared **in source syntax**, so a
trait declaring `const LIMIT: Self::Item` is offered `const LIMIT: Self::Item =
<value>`, in the two segments a projection is written with and never in the
compiler's own `<Self as Source>::Item`, which is E0101 where it is offered.
Defining one
twice is E0426. Defining one the implemented trait does not declare is E0425,
and so is writing any associated item in an inherent `impl Counter { ... }`,
which declares nothing: the only home the language offers an associated item is
the impl of a trait that declares it.

The constant's value is folded at compile time, so an expression is allowed
where it folds, and `const LIMIT: int = 2 + 1` is accepted. Three shapes are
E0422, and they are distinct: a declared type that differs from the trait's,
as in `const LIMIT: string` against the trait's `const LIMIT: int`, which names
both, the trait's side substituted as this impl instantiates it; an
initialiser whose type differs from the declaration the impl itself wrote, as in
`const LIMIT: int = "x"`; and an initialiser no rule gives a type at all, as in
`const LIMIT: int = vec![1, 2]`. The third names no type for the
initialiser, because none was established; only the first two can say what the
initialiser produced. E0422 reaches associated constants only, since an
associated type has no declared type to disagree with. An item the impl defines
twice is E0426 and its initialiser is not examined, since neither of the two
definitions is the one the impl meant.

### Projections

A projection is exactly two segments, a receiver and an item name, written
`Receiver::Name`. The accepted receivers:

| Receiver | Example | Resolves to |
|----------|---------|-------------|
| a nominal type | `Counter::Item`, `Counter::LIMIT` | the item of whichever impl for `Counter` defines it |
| a generic nominal type, written bare | `Wrap::Item`, `Wrap::LIMIT` | the item of the single impl for `Wrap`, in both namespaces; two instantiations of `Wrap` that each define the item are E0423 |
| a trait | `Source::Item` | the item of the single impl of `Source` that defines it |
| `Self` | `Self::Item`, inside a trait declaration or an impl | the item of the implementing type |
| a bounded type parameter | `T::Item` under `T: Source` | the item of the impl selected at the call site |

`Self` outside an impl and outside a trait declaration is E0423, and a type
parameter whose bounds declare no such item is E0423 asking for the bound.

A generic receiver is written **without its arguments**. `Wrap::Item` and
`Wrap::LIMIT` both name the impl for `Wrap`, whether that impl is written
`impl<T> Source for Wrap<T>` or `impl Source for Wrap<int>`, and they answer the
same inside a method body as in the signature above it. The one thing the bare
receiver cannot reach is an associated type the impl defines *as* one of its own
parameters, as in `impl<T> Source for Wrap<T> { type Item = T }`: the receiver
says nothing about `T`, so `Wrap::Item` is E0423 saying exactly that. There is no
spelling that supplies the arguments: `Wrap::<int>::Item` is E0101 in a type,
a parameter and an array length alike, and `Wrap<int>::Item` is E0101 anywhere.
The turbofished form parses in **value position only**, where its arguments
select nothing at all: `Wrap::<int>::LIMIT`, `Wrap::<string>::LIMIT` and
`Wrap::<Nope>::LIMIT` all read the same constant, so the bare spelling is the
one to write. Because no spelling supplies the arguments, one trait implemented
for two instantiations of one type constructor, `impl Source for Wrap<int>` and
`impl Source for Wrap<string>` both defining `LIMIT`, leaves the receiver naming
two definitions and is E0423 rather than an answer picked by file order.

A **built-in type is not a receiver**. `int::Item`, `string::Item` and
`Vec::Item` are E0423 saying that a built-in type can hold no impl and so
declares no associated item; they are not unknown types. Only a struct, an enum,
a trait, `Self`, or a bounded type parameter names associated items.

A **module-qualified receiver is not implemented**. `support::Counter::Item` is
E0372 `unknown type 'support'`, and so is `support::Counter` on its own: a module
path names no type in this language, whichever `needs` form imported the module.
The bare receiver is the working spelling, and it works under every import form,
selective, whole-module, and aliased, because an import brings the type itself
into scope rather than a path to it.

A projection fails to resolve for these reasons, all E0423:

- no impl for the receiver defines the name;
- the receiver is a built-in type, which can hold no impl;
- the name exists in the other namespace, which the message says outright,
  naming the namespace written and the one the position wanted;
- the impl defines the item as one of its own type parameters, and the bare
  receiver says nothing about that parameter;
- the receiver is ambiguous, in one of three shapes. **Two traits, one type**:
  `Counter::Item` where two impls for `Counter` define `Item`. The repair that
  changes nothing else is to name the trait, `Alpha::Item`, and it works in both
  namespaces. **One trait, two types**: `Source::Item` where two types implement
  `Source`. There the repair is to name the type, `Counter::Item`; renaming the
  item would change nothing, since a single trait already provides it. **One
  trait, one type constructor, two instantiations**: `Wrap::LIMIT` where
  `impl Source for Wrap<int>` and `impl Source for Wrap<string>` both define
  `LIMIT`. Neither of the first two repairs applies, since there is one trait and
  one type, and a projection writes `Wrap` without its arguments; the repair is
  to keep a single impl of the trait for that type constructor, or to declare the
  item in a second trait and name that trait. `Source::LIMIT` and `Wrap::LIMIT`
  reach the same sentence. The three shapes carry different sentences;
- the definition resolves back to itself. A cycle is rejected when the impls are
  collected, so it is a diagnostic whether or not any program names the
  projection;
- the constant is defined but its value cannot be computed, because the
  arithmetic overflows or divides by zero;
- the constant is defined but its initialiser is not a constant integer
  expression. A constant is declared only with a type the compiler folds a
  constant of, which E0434 enforces at the declaration and at each impl, so the
  declared type is one of those and the message prescribes integer literals.
  Where the foldable set widens, a constant of a type the folder accepts but
  this initialiser does not produce names the declared type instead, in the
  spelling the impl wrote it;
- `Self` is written where no impl and no trait declaration is open;
- the receiver is a type parameter whose bounds declare no such item, or a
  parameter of a struct or an enum, which carries no bound at all;
- a fixed-array length is taken from a type parameter.

A length that resolves and is **negative** is not in that list: the projection
resolved, so it is E0315, the same code and the same sentence a negative literal
length takes, naming the constant that produced it.

Because a nominal receiver and a trait receiver share one spelling, a trait may
not share its name with a struct or an enum; that is E0326, stated above under
[Traits](#traits).

### Where a projection may be written

A projection is written where its namespace belongs, and nowhere else. The rule
is uniform: **every type position accepts the type and refuses the constant, and
every value position the reverse.** A bound is one role holding both, and it takes
its namespace from the position like every other role: a type on the right
selects the associated type and a value the associated constant, so
`Source<Item = Counter::Item>` and `Source<LIMIT = Counter::LIMIT>` are both
accepted and both crossings are E0423. A trait declaring one name in both
namespaces therefore admits both spellings of a binding on it, each checked
against the item its own side names. The compiler names the position it
refused in, and the names it can print are fixed: `OccurrenceRole` in
`sema/src/infer.rs`
enumerates a parameter type, a return type, a struct field, an enum variant
field, a bound, an impl header and an associated item definition, and a
projection written outside any of them is reported as a type annotation, a value
expression or an array length. The grid below is that enumeration, and every
refusal is E0423 naming the mismatch:

| Position | `Counter::Item`, a type | `Counter::LIMIT`, a constant |
|----------|-------------------------|------------------------------|
| variable annotation, at top level or in a body | accepted | E0423 |
| parameter type | accepted | E0423 |
| return type | accepted | E0423 |
| struct field type | accepted | E0423 |
| enum variant payload | accepted | E0423 *(an enum variant field)* |
| enum struct-variant field | accepted | E0423 *(an enum variant field)* |
| an associated type binding in a bound, `Source<Item = ..>` | accepted | E0423 *(a bound)* |
| an associated constant binding in a bound, `Source<LIMIT = ..>` | E0423 *(a bound)* | accepted |
| impl header target | resolves, then E0339 for a non-local target | E0423 *(an impl header)* |
| associated item definition | accepted | E0423 *(an associated item definition)* |
| the declared type of an associated constant | accepted | E0423 *(an associated item definition)* |
| array element type, nested to any depth | accepted | E0423 |
| a generic type argument, `Holder<Counter::Item>` | accepted | E0423 |
| an `fn(..) -> ..` annotation, either half | accepted | E0423 |
| value expression | E0423 | accepted |
| fixed-array length | E0423 | accepted |
| array repeat count | E0423 | accepted |
| lambda body | accepted, as an annotation | accepted, as a value |
| across an imported module | accepted | accepted |

An impl header is the one row where acceptance is not the end of the story:
under `type Item = int`, `impl Mark for Counter::Item` resolves the projection to
`int` and is then refused by E0339, because a trait may not be implemented for a
type that is not local. The projection did its work; the orphan rule is a
separate refusal. Under `type Item = Payload`, a struct or an enum the program
declares, the same header resolves to that local type and the impl is registered
for it, so `Payload { n: 6 }.tag()` runs the body written under the projected
header. The header names the type the item resolves to, never the item's own
name, in the diagnostics and in the symbols alike.

The last row is not a weaker case. Every position above holds for a type
imported from another module, the struct field and both halves of a signature
included.

Three traps turn on what the receiver is known to be at that point rather than
on the position:

- Inside a generic body, `T::Item` is **not** the concrete type it will become.
  A value annotated `T::Item` is not interchangeable with an `int` even when
  every call site instantiates `T::Item = int`: both `x + 1` and `let y: int = x`
  are E0301. Return it, pass it, or store it, and it resolves at the call site.
- A fixed-array length may not come from a type parameter, so `[int; T::LIMIT]`
  is E0423 even under `T: Source`. Inside a **trait declaration** `Self` is such
  a parameter, so `[int; Self::LIMIT]` in a trait method signature is E0423
  whether the constant is declared on that trait or inherited from a supertrait.
  That is a property of the position, not supertrait blindness: the same length
  is accepted in a default body of the same trait, and in an impl's signature
  and body alike, where `Self` is one concrete type. The repairs the diagnostic
  offers are a literal length, the constant on a concrete type, or a `Vec`.
- A method that declares its **own** type parameter is monomorphized over the
  nominals the way a free generic function is: one body per reachable instance,
  so a constant on that parameter reads the value of that instance.
  `fn pick<T: Source>(self, v: T) -> int { T::LIMIT }` inside an `impl` reads
  `LIMIT` from the impl each call site selects, and a bound declared on that
  parameter is checked against the argument bound to it. A call site that binds
  nothing to the parameter is E0343, the same refusal the free generic twin
  gives.
- A method's own type parameter spelled like one the **receiver's type** declares
  is not a second, free parameter at a call. The receiver fixes that type's
  parameters and the binding stands: `impl<A> P<A> { fn get<A>(self) -> A }`
  called on a `P<int>` answers `int`, so a caller that declared `bool` is E0301,
  and an argument annotated with that parameter is read at the receiver's type
  rather than at its own. The spelling that collides is the one the `struct` or
  `enum` declares, which an impl need not reuse: with `struct P<A>`, an
  `impl<Z> P<Z>` whose method declares `A` collides all the same. A parameter the
  call site deduces on its own needs a spelling the nominal does not use. A bound
  written on the method's parameter is still the method's, and does not reach the
  impl's. Reached through a bound instead of a receiver, none of this applies:
  there is no receiver to fix anything, and the two parameters are not told apart.
- The resemblance stops at calls out to a free generic function. Inside an
  `impl` or a trait default body, a call to a free generic function whose type
  argument is, or contains, a type parameter of the enclosing method or impl is
  E0343, and the turbofish the diagnostic asks for does not lift it:
  `fn grow<T>(self, v: T) -> string { ident(v) }` and the same body written
  `ident::<T>(v)` are both refused, while that body written as a free generic
  function runs. The refusal is on the body rather than on a call site: it
  stands when the method is never called, and a concrete call to the same free
  function elsewhere in the program does not supply the instance. What passes
  from those bodies is every call whose type argument is already concrete:
  `ident(7)`, `ident(self.n)`, `ident::<int>(7)`, a call to another method, and
  a call to a free function that is not generic. A generic `impl` header on its
  own changes nothing; only an argument or a return carrying the parameter
  does.

### Bindings in a bound

A bound may pin the associated items of the trait it names, in the inline form
or in a `where` clause:

```rust
trait Source {
    type Item
    const LIMIT: int
    fn next(self) -> Self::Item;
}

struct Counter { value: int }

impl Source for Counter {
    type Item = int
    const LIMIT: int = 3
    fn next(self) -> int { self.value }
}

fn take<T: Source<Item = int, LIMIT = 3>>(value: T) -> int { T::LIMIT }
fn also<T>(value: T) -> int where T: Source<Item = int, LIMIT = 3> { T::LIMIT }

println(take(Counter { value: 1 }) + also(Counter { value: 1 }))
```

The angle brackets hold associated bindings, not type arguments. A binding is
checked against the impl selected at the call site, and four things go wrong:

- the impl provides something else. `Item = string` against an impl providing
  `int`, or `LIMIT = 4` against an impl providing `3`, is E0424, which prints
  both sides;
- the trait declares no such item. `Nope = int` is E0424 saying the binding
  constrains nothing, reported on the bound itself: an uncalled function whose
  bound names nothing the trait declares is refused all the same;
- the binding crosses the namespaces. `Item = 3` binds a value to an associated
  type and `LIMIT = int` binds a type to an associated constant, both E0424,
  reported on the bound itself rather than at the call site. A crossing needs
  the trait to declare that name in the other namespace **alone**: where the
  trait declares it in both, the right side decides and neither spelling
  crosses;
- a side has no value the compiler can compute. Against an impl whose
  `const LIMIT: int = 1 / 0`, the comparison never happens, and reporting a
  disagreement would assert a cause nobody established. That is E0429, distinct
  from E0424 for exactly that reason. The requested side of a binding is a
  literal, `LIMIT = 1 / 0` being E0101, so only the impl's side can reach it.

The object form of a binding, `dyn Source<Item = int>`, is not available: `dyn`
is E0113, deferred, as [Stage 3 boundary diagnostics](#stage-3-boundary-diagnostics)
records.

### Inherited associated items

A supertrait's associated types and constants are in scope wherever its methods
are: the declaration of the inheriting trait, a default body, an impl of it, and
a bound on a type parameter. They are *defined* in the impl of the trait that
declares them, and writing one in the impl of an inheriting trait is E0425.

```rust
trait Base {
    type Item
    const LIMIT: int
    fn base(self) -> Self::Item;
}

trait Derived: Base {
    fn extra(self) -> Self::Item { self.base() }
    fn cap(self) -> int { Self::LIMIT }
}

struct Bullet { n: int }

impl Base for Bullet {
    type Item = int
    const LIMIT: int = 4
    fn base(self) -> int { self.n }
}
impl Derived for Bullet { }

println(Bullet { n: 3 }.extra() + Bullet { n: 3 }.cap())
```

The two halves hold each other up. Because an impl of a trait obliges an impl of
each of its supertraits, the impl that must define `Item` always exists, so
E0421 has an impl to attach to and the inheriting impl never needs a second home
for the same item.

### Nominal types no value inhabits

`struct A { a: A }` describes a type whose construction needs a value of itself
before it can start, so no program can build one. The definition is rejected
with E0428 rather than accepted and left unusable.

The criterion is inhabitation, not size. The **constructor graph** has an edge
from a struct to each field type, from an enum to each payload type of each
variant, and from `[T; n]` to `T` only when `n > 0`. `Option`, `Vec` and
`Result` contribute no edge: each has a constructor that is inhabited whatever
its type arguments are. The **fixpoint**: a struct is inhabited when every field
type is, an enum when at least one variant is, and `[T; n]` when `n == 0` or `T`
is. **E0428 fires when the fixpoint fails for a type that lies on a cycle of
that graph.** The message names the cycle path, and the accused definition is
chosen by a property of the cycle rather than by declaration order, so the two
orders of a two-node cycle produce the same sentence.

Rejected: `struct A { a: A }`, `enum E { V(E) }`, `struct A { xs: [A; 1] }`, and
a struct and an enum that carry each other. Accepted, and each of these
executes: `enum List { Cons(int, List), Nil }`, `struct A { f: Option<A> }`,
`struct A { xs: Vec<A> }`, `struct A { r: Result<A, int> }`, and
`struct A { xs: [A; 0] }`. A type whose fixpoint fails without lying on a cycle
is also accepted: `enum E { }` is a construct written on purpose, and so is a
struct that carries one. E0428 is named for the cycle and is not a general
uninhabitation ban.

### Impl type parameters

An impl method's symbol is mangled from three names, each written as the byte
length of the text in eight hexadecimal digits, a colon and the text: the trait,
the **header spelling** of the target type, and the method. The header spelling
keeps the target's arguments, with the impl's type parameters numbered by first
appearance, so `impl Source for Wrap<int>` mangles `next` to
`__aelys_trait::00000006:Source00000009:Wrap<int>00000004:next` and
`impl Source for Wrap<bool>` to a second symbol. The trait's own type arguments
follow in the same encoding, and a generic impl then carries one suffix per
instance holding the substituted target type. Two impls of one trait for two
instantiations of one constructor are therefore two symbols, and both are
licit; E0355 is left to two instances that still collide after this spelling.

A call on a receiver of that constructor, when several impls supply the method,
is typed through the **trait's declaration** with `Self` read as the receiver if
the trait takes no parameters: an impl conforms to the trait position by position,
so the declaration is what any of them answers. An associated type in that
signature stays a projection on the receiver until the receiver is known, and is
then read from the impl that will run, the most specific one when several
overlap. Nothing is refused for the impls being several, whether they are
disjoint, form a specialization chain, or both; the monomorphizer runs, for each
instance, the impl that covers its receiver, and refuses with E0338 an instance
that no impl covers.

An impl that no instantiation of the receiver reaches is set aside, in a
generic body or a trait's default body as well as on a concrete receiver. When
no impl that supplies the method is left, no instance of the call can succeed,
and it is refused with E0338 where it is typed, naming the trait and the
receiver; this holds for a method call, a call whose receiver is known only at
its instance and a qualified call alike, whether one impl or several supply the
method. When one impl alone is left and it covers the receiver whatever the
receiver's own type parameters become, it runs for every instance: its header
binds the receiver and its own signature, associated types included, types the
call. One that reaches the receiver without covering it runs for some instances
only: for a trait without parameters the call is typed through the trait and
each instance takes the impl that covers it or is refused with E0338, and for a
trait that takes parameters the call is refused.

A trait that takes parameters is typed through its declaration only when the
impl chosen is the root of a specialization chain: the root covers every
receiver the chain covers, so its header, equated with the receiver, fixes the
trait's arguments, and an associated type in the signature stays a projection
until the impl that runs answers it. The root is looked for among the impls the
receiver has not set aside, so two chains on one nominal each keep their own,
and a trait's default body adopted by an impl of either chain is typed. On a
receiver whose type parameters stay open, as in a generic body, such a
projection is read from the most specific impl that covers the receiver, when
every impl that reaches the receiver, that it does not outrank, and that no more
specific impl covers wherever it applies, gives under that instantiation what it
gives there: `N<X, int>::Out` is
`X` when the root answers `A` and a specialization for `N<int, B>` answers
`int`, and `N<int, X>::Out` is that specialization's answer, which runs for
every instance. Otherwise it stays opaque, and a
body that needs it to be one particular type is refused, even when the
instances the program creates would all have given that type. Otherwise, when the impls give the trait
different arguments, the call cannot name which instantiation it means, which is
E0437, and its message lists every impl that supplies the method, header
included. Two impls give the trait the same arguments when each argument is the
same type, or the same position of the receiver: the names an impl gives its
parameters, and the order the file declares the impls in, decide nothing. When
the arguments are the same and the receiver is still open, the impl cannot be
chosen before the receiver's instantiation is known, which is E0438; annotating
the receiver chooses it. A receiver its impl header does not fit is refused.

A qualified call, `Tr::m(p)`, reads the impl the receiver leaves standing, with
that impl's parameters instantiated afresh at each call, and each instance
chooses its impl from the receiver again, as a method call does: a negative
impl that denies the trait to the receiver refuses it with E0338. On a
concrete receiver that several impls still apply to, a qualified call is
refused with E0443 when they give the trait one instantiation, and with E0437
when they do not. A qualified call on a receiver still open over a
specialization chain is not supported and is refused with E0352. A receiver known only once its instance is, as `id(N { .. })`,
calls a method that has its own type parameters as a method call does: each
call site gives those parameters its arguments, and every argument the site
typed must be what the method takes, whether or not its result is used. An
argument that is itself such a call is resolved first, so what it returns is
what the method is given. Where
several impls could run, the impl is chosen once the receiver is known, and a
receiver in a lattice left open is E0443 there as on a method call. Such a call
sees the impls a method call sees: in the program's root module, an impl of a
trait the root cannot see supplies nothing, and the call is refused as a member
the type does not have. A module's bodies are typed inside the program that
imports them and see every impl that program declares, the importer's own
traits included, so a method one of them shares with the module's traits is
ambiguous there (E0337).

Inside an impl, in its signature as in its bodies, `Self::Item` is read by the
same rule, on the impl's own header: it is that impl's answer unless another
impl that reaches some of its receivers, that it does not outrank, and that no
more specific impl covers wherever it applies, answers otherwise there. When
two of them define it differently, as a specialization that redefines an
associated type does, `Self::Item` is not one type across those receivers and
is refused with E0423, whose message lists the impls that reach them; the impl
writes the type itself instead. An annotation in a body names `Self` as that
header, not as the nominal alone, and a projection under a constructor, as
`W<Self::Out>`, is read where the call is typed, so each instance materializes
the constructor.

Two impls that overlap without one being strictly more specific (E0340), or
that are equally specific (E0442), are both withdrawn, and an impl withdrawn
this way still meets the impls that follow it, so the impls withdrawn do not
depend on the order the file declares them in. These verdicts are reported
before any error the withdrawal causes elsewhere in the program, and errors of one
kind that a trait's default body raises once per impl adopting it are reported
in the order of what they say rather than the order of the impls.

A trait's default body is typed once per impl that adopts it, and a call on
`Self` there reaches the trait at the instantiation that impl gives it: another
impl of the trait at another instantiation, even one whose header reaches the
same receiver, is not a candidate, since each instance runs the adopting impl's
own methods.

A call through a bound on a type parameter reads the impls by what they give
the trait at the receiver, and takes the most specific only where it refines
the others, header and trait arguments together, as a method call on that
receiver does; where it does not, no call can name one, which is E0437, and
where two incomparable impls both apply it is E0443. A pair whose headers are
ordered but whose trait arguments are not is refused at every use, whichever
route the call takes.

A negative impl supplies nothing: none of its trait's default bodies is typed on
its header.

`impl<T, U> W<T>` is rejected at the header with E0430, the caret on the
target type, naming the parameter and the two repairs: mention it in the target
type, or move it onto the method. The constrained set is exactly "appears in the
impl target type", which deliberately excludes a parameter appearing only in a
trait argument, only in a method signature, or only in a `where` clause.

### The diagnostics

Ten codes carry this material. The registry gives each its name; the condition
is here.

Every type any of them names is written in the spelling a program could have
written: `int` and not `i64`, `float` and not `f64`, `Vec<int>` and not
`vec[i64]`, `fn(int) -> int` and not `(i64) -> i64`, `Counter::Item` and not
`<Counter as Source>::Item`. A message that told the reader to write a type
would otherwise offer one the grammar refuses.

- **E0421** fires when an impl of a trait omits an associated type or constant
  the trait requires.
- **E0422** fires when an impl's associated constant declares a type the trait
  does not, is initialised with a value of a type its own declaration does
  not, or is initialised with an expression whose type no rule establishes. In
  the last shape the message states that no type was established rather than
  naming one.
- **E0423** fires when a projection resolves to nothing, to more than one thing,
  or to itself: no impl defines the name, the receiver is a built-in type, the
  name is in the other namespace, the impl defines the item as one of its own
  type parameters, two impls define it, the definition cycles, the constant is
  defined but cannot be folded or is not a constant integer expression, `Self`
  is written where no impl or trait declaration is open, the receiver is a
  parameter no bound covers, or a fixed-array length is taken from a type
  parameter. A length that folds to a negative value is **not** E0423: the
  projection resolved, so it is E0315.
- **E0424** fires when an associated binding in a bound disagrees with the
  selected impl, names an item the trait does not declare, or binds a value to a
  type or a type to a value. The last two are decided by the bound alone, so
  both are reported where the bound is written, whether or not anything calls
  the function; only the disagreement with a selected impl waits for an
  instantiation.
- **E0425** fires when an impl defines an associated item the trait it
  implements does not declare, and when an inherent impl defines one at all.
- **E0426** fires when one impl defines the same required associated item more
  than once.
- **E0427** fires when resolving a projection expands past 4096 type nodes. The
  bound is on the **size of the expanded type**, not on the number of resolution
  steps: termination is already guaranteed by cycle detection, and a step budget
  would reject a long acyclic chain while admitting a short explosive one. An
  associated type naming another one twice, as in
  `type I0 = Result<Self::I1, Self::I1>`, doubles the expansion at each step and
  reaches the bound within a few dozen lines. The same chain is E0427 at the
  same depth in a variable annotation, a parameter type, and a struct field.
- **E0428** fires when the inhabitation fixpoint fails for a nominal type lying
  on a cycle of the constructor graph.
- **E0429** fires when an associated-constant binding has a side the compiler
  cannot fold, so no comparison happened.
- **E0430** fires when an impl header declares a type parameter its target type
  never mentions.
- **E0434** fires when an associated constant is declared with a type the
  compiler folds no constant of, and names the foldable set in the message
  rather than a fixed sentence, so a later stage that folds another kind widens
  the set and the diagnostic follows. Today the folder yields an `i64`, so
  `int` is the whole set. It is checked wherever the type is settled. **At the
  trait declaration** a projection is resolved first and then checked, since it
  is settled there, and any other type that is neither a type parameter nor an
  unsettled projection must fold. **At each impl** the trait's declared type is
  substituted, the parameter by the argument the impl instantiates and `Self` by
  the target; the result must fold, and the refusal lands on the impl's own
  definition naming the **instantiated** type. So
  `trait Bounds<T> { const LIMIT: T; }` with `impl Bounds<int>` reads back and
  the same trait with `impl Bounds<string>` is E0434 at the impl, and
  `trait T3 { const Y: Counter::Item; }` where `Counter::Item` is a `string` is
  E0434 at the declaration. Nothing declarable is left definable and readable
  nowhere.

## Compiler Warnings

The compiler can emit warnings for various situations. Warnings don't stop compilation but indicate potential issues :

### Warning Categories

| Code | Category | Description |
|------|----------|-------------|
| W01xx | inline | Issues with `@inline` or `@inline_always` |
| W02xx | unused | Unused variables, functions, imports |
| W03xx | deprecated | Deprecated features or functions |
| W04xx | shadow | Variable shadowing |
| W05xx | type | Unknown types and suspect comparisons |

### Inline Warnings

```rust
// W0101: can't inline recursive functions
@inline
fn factorial(n: int) -> int {
    if n <= 1 { return 1 }
    n * factorial(n - 1)
}
```

The compiler warns because inlining a recursive function would cause infinite expansion. Remove `@inline` or break the recursion.

Here's some other inline warnings:

- **W0102**: Mutual recursion (A calls B, B calls A)
- **W0103**: Function captures variables from outer scope
- **W0104**: Public function is being inlined (original kept for external callers)
- **W0105**: Native function can't be inlined

### Warning Flags

Control warnings from the command line:

```bash
# enable all warnings
aelys compile main.aelys -Wall

# treat warnings as errors
aelys compile main.aelys -Werror

# enable a specific category
aelys compile main.aelys -Winline

# disable a category
aelys compile main.aelys -Wno-inline

# combine: all warnings, but not unused, treat as errors
aelys compile main.aelys -Wall -Wno-unused -Werror
```

The `-Werror` flag is useful in CI to catch issues early

## Error Handling

Errors are values. A fallible function returns `Result<T, E>` and an optional value
returns `Option<T>`:

```rust
fn read(text: string) -> Result<int, Error> {
    match convert::parse_int(text) {
        Some(value) => Ok(value),
        None => Err(Error::Message("not a number")),
    }
}

fn doubled(text: string) -> Result<int, Error> {
    let value = read(text)?
    Ok(value * 2)
}

println(match doubled("21") { Ok(value) => value, Err(error) => 0 })   // 42
println(match doubled("x") { Ok(value) => value, Err(error) => -1 })   // -1
```

`?` follows the residual, not the wish. `convert::parse_int` returns
`Option<int>`, so applying `?` to it inside a `Result`-returning function is
E0373 and the match above is what converts the absence into an error. In an
`Option`-returning function the same `?` is exactly right:

```rust
fn parse(text: string) -> Option<int> {
    let value = convert::parse_int(text)?
    Some(value + 1)
}

println(match parse("41") { Some(value) => value, None => 0 })    // 42
println(match parse("nope") { Some(value) => value, None => 0 })  // 0
```

`match` on `Option` and `Result` is exhaustive. Omitting a variant is a compile
error. A named `Option` or `Result` cannot be silently discarded: use it, return it,
match it, call a consuming method, or write `let _ = expression` explicitly. That
last spelling is the only discard; there is no `discard` keyword. E0303 (Result) and
E0304 (Option) print these repairs. The check covers expression statements, block
and closure bodies, match arms, and named bindings; a `Result` stored inside a
collection literal such as `vec![r(), r()]` is not tracked.
Assigning one to a variable, a field or an element (`a.next = Some(x)`,
`v[i] = Ok(1)`) uses it. A function or lambda without a declared return type
whose last statement is an assignment returns unit, so a setter such as
`fn link(mut self, v: int) { self.next = Some(v) }` can be called as a statement.

The `?` operator propagates `Err` or `None` out of the enclosing function and
otherwise yields the operand's success value. The success type of the operand and
the success type of the function are independent: inside a function returning
`Result<U, F>`, `expr?` with `expr: Result<T, E>` has type `T` whatever `U` is, and
the same holds for `Option<T>` inside a function returning `Option<U>`:

```rust
fn port(text: string) -> Result<int, string> {
    match convert::parse_int(text) { Some(n) => Ok(n), None => Err("bad port {text}") }
}

fn configure(text: string) -> Result<unit, string> {
    let p = port(text)?          // Result<int, string>? is an int here
    println("listening on {p}")
    Ok(())
}
```

`?` is valid inside functions, methods, and closures. At the top level of a script
there is no function to return from, so `?` there is E0305 with a help line pointing
at `match`, `unwrap_or`, or `expect`. `?` on a value that is neither an `Option` nor
a `Result` is E0306.

The available methods are `unwrap`, `expect`, `unwrap_or`, `unwrap_or_else`, `ok`,
`err`, `map`, `map_err`, `and_then`, `or_else`, and the tests `is_ok`, `is_err`
(Result) and `is_some`, `is_none` (Option), which return `bool`. `opt.ok_or(e)`
turns an `Option<T>` into a `Result<T, E>`, with `e` evaluated eagerly. There is no
`unwrap_or_default`: the language has no general notion of a default value.
There is no null literal or null-inspection builtin in the surface language.
The closed built-in `Error` family has one data-carrying constructor,
`Error::Message(string)`.

### Panics

A panic stops the program. It never unwinds, never turns into an `Err`, and `?`
cannot catch it. Four operations panic:

- `panic(message: string)` has type `never`, so it fits any expression position,
  such as a match arm whose other arms produce an `int`;
- `assert(condition: bool, message: string)` panics with
  `assertion failed: message` when the condition is `false`;
- `unwrap()` on `None` or `Err`, and `expect(message)` on the same values.

`panic` and `assert` print `panic: <message>` to stderr, followed by the source
location and the stack trace, and the command-line runner exits with status 101.
`unwrap` and `expect` failures print the error payload, as
`Result::unwrap failed on Err: <payload>` or `<message>: <payload>`, followed by the
location and the stack trace, and exit with status 1. A string payload prints as
is; any other payload prints in a debug form such as `WadError::Short(3, 4)` or
`Point { x: 1, y: 2 }`. All of these behave the same under `-ae.jit=on`.

```rust
fn lump_size(kind: int) -> int {
    match kind {
        0 => 4,
        1 => 8,
        _ => panic("unknown lump kind {kind}"),
    }
}
```

A program can shadow `panic` or `assert` with its own declaration of the same name.

### Error conversion through `?`

The prelude declares the compiler-known trait `From<Source>` with the associated
function `from(Source) -> Self`. The identity conversion is a compiler rule rather
than a registered impl, and the standard library provides the single
`From<string> for Error` implementation. User impls are ordinary coherent `From`
impls, checked by the same orphan and overlap rules as every other trait:

```rust
struct ParseErr { line: int }
struct AppErr { line: int }

impl From<ParseErr> for AppErr {
    fn from(source: ParseErr) -> AppErr { AppErr { line: source.line } }
}

fn parse() -> Result<int, ParseErr> { Err(ParseErr { line: 3 }) }

fn run() -> Result<int, AppErr> {
    let value = parse()?
    Ok(value)
}
```

`from` is an ordinary associated function, so it can also be called by name
outside any `?`. The word is a keyword only inside a `needs ... from ...` clause:

```rust
let converted = AppErr::from(ParseErr { line: 3 })
```

For `Result<T, E>?` in a function returning `Result<U, F>`, the operand must be a
`Result`, the expression has type `T` with no relation required between `T` and
`U`, and either `E` equals `F` or exactly one `From<E> for F` impl is selected. Selection tries the identity rule
first and then the non-identity impls; the failure path calls the selected `from`
symbol directly and returns `Err`. It never stringifies the error and never calls a
runtime converter.

Converting through `From` is a deliberate deviation from a "no hidden conversion"
rule: the impl is always written out by the user, is selected at compile time, and
is never replaced by a runtime converter.

`Option<T>?` is valid only in an `Option<U>` context, has type `T`, and propagates
`None`. Mixing
an `Option` residual with a `Result` return, or the reverse, is `E0373`. A missing
or ambiguous conversion is `E0374`, which names both types, the candidate impls,
and `map_err` as the repair. Every `(From, T, T)` header is reserved for the
compiler rule, so a user impl that unifies with it is rejected with `E0375` and can
never shadow or duplicate the identity conversion.

## Future Plans

These features are outside the delivered language surface:

- trait objects and dynamic dispatch
- higher-kinded types
- generic function values without an explicit concrete instantiation
- async/await

Negative impls (`impl !Trait for Type`) and specialization (`default fn`) are
**delivered**, with their diagnostics E0431 to E0433 and E0441 to E0444 below.

### Stage 3 boundary diagnostics

Five constructs parse but are refused with a diagnostic that names them as
deferred rather than as unknown syntax, so that a program written against a later
stage fails with a clear reason instead of a parse error.

| Code | Construct | Message |
|------|-----------|---------|
| E0111 | `&self`, `&mut self` | borrowing receiver '&self' is deferred to Stage 3 |
| E0113 | `dyn Trait` | trait object 'dyn T' is deferred to Stage 3 |

Each carries the repair for the current stage. A method takes its receiver by
value, so write `self`; a trait or impl body holds only `fn` items; a trait object
becomes a generic parameter bound by that trait; a negative impl has no Stage 2
spelling; and specialization becomes one plain `fn` per impl.

```rust
struct Point { x: int }
impl Point { fn get(&self) -> int { self.x } }
// ✗ error[E0111]: borrowing receiver '&self' is deferred to Stage 3
//   help: Stage 2 methods take the receiver by value; write 'self'
```

Note that `dyn` is contextual: it is a trait object marker in a type position and
an ordinary identifier everywhere else.

## Diagnostic Codes

Every compile diagnostic carries a stable numeric code, printed as `E` followed
by four digits. This is the registry. It is generated from two places in the
source, `CompileErrorKind::code` in `common/src/error/compile/code.rs` and
`TypeErrorKind::diagnostic_code` in `sema/src/constraint/error.rs`, and a test
fails if a code here does not exist in the source, if a code in the source is
missing here, or if either source file hands the same number to two different
diagnostics.

A handful of numbers appear in both source files. That is deliberate: the two
enums name the same condition at two stages of the pipeline and share its code.

### E00xx, lexical

| Code | Name | Meaning |
|------|------|---------|
| E0001 | UnterminatedString | a string literal reaches end of input |
| E0002 | InvalidCharacter | a character that starts no token |
| E0003 | InvalidNumber | a numeric literal that does not parse |
| E0004 | CommentNestingTooDeep | block comments nested past the limit |
| E0005 | InvalidEscape | an unknown escape sequence in a string |
| E0006 | UnterminatedFmtExpr | an interpolation `{` with no closing `}` |
| E0007 | UnmatchedCloseBrace | a `}` with no interpolation to close |
| E0008 | InvalidCharLiteral | a char literal that is empty, holds more than one character, is unterminated, or a `\u{...}` escape naming no scalar value |

### E01xx, syntax

| Code | Name | Meaning |
|------|------|---------|
| E0101 | UnexpectedToken | a token the grammar does not allow here |
| E0102 | ExpectedExpression | an expression was required |
| E0103 | ExpectedIdentifier | an identifier was required |
| E0104 | InvalidAssignmentTarget | the left side of `=` is not a place |
| E0105 | RecursionDepthExceeded | the parser exceeded its nesting limit of 256 levels: each parenthesis, call, prefix operator, assignment, block and pattern level counts one, and so does the expression that holds them, so a `let` initializer accepts 255 nested parentheses, a call argument 254, and 254 nested `if` blocks |
| E0106 | NullIsNotInSurface | `null` is not part of Aelys |
| E0107 | ExpectedPattern | a pattern was required |
| E0108 | InvalidPattern | a pattern the grammar does not allow |
| E0109 | UnknownVariant | no such variant on that type |
| E0110 | MatchArmValueRequired | a match arm whose block ends without a value, in a match whose other arms produce a non-`unit` value |
| E0111 | BorrowingReceiverDeferred | `&self` and `&mut self` are Stage 3 |
| E0113 | TraitObjectDeferred | `dyn Trait` is Stage 3 |
| E0114 | ReservedIdentifier | a declared name starts with `__`, which is reserved for the compiler and its builtins |

### E02xx, names and emission limits

| Code | Name | Meaning |
|------|------|---------|
| E0201 | UndefinedVariable | no such name in scope |
| E0202 | VariableAlreadyDefined | a name is declared twice in one scope |
| E0203 | AssignToImmutable | assignment to a binding without `mut` |
| E0204 | TooManyConstants | the constant pool limit is exceeded |
| E0205 | TooManyRegisters | the register limit is exceeded, or an instruction with no wide form would need a register above 255; reported at the line that needs it, never compiled into a truncated register |
| E0206 | TooManyArguments | the argument limit is exceeded |
| E0207 | BreakOutsideLoop | `break` outside a loop |
| E0208 | ContinueOutsideLoop | `continue` outside a loop |
| E0209 | IntegerOverflow | an integer literal or fold overflows |
| E0210 | AssignToLoopVariable | assignment to the loop variable |
| E0211 | TooManyUpvalues | the upvalue limit is exceeded |
| E0212 | ReturnOutsideFunction | `return` outside a function |
| E0213 | JumpOffsetTooLarge | a jump exceeds the encodable range |
| E0214 | CompilationLimitExceeded | a compilation limit is exceeded |
| E0215 | MissingReturnValue | a path falls through without a value |
| E0216 | CallWindowOverlap | internal compiler error: a call frame would overwrite a register that is still live; the program is refused instead of run with a corrupted local |

### E03xx, types

| Code | Name | Meaning |
|------|------|---------|
| E0301 | TypeInferenceError | a type mismatch or an unresolved name |
| E0302 | NonExhaustiveMatch | a match is missing a constructor or `_` |
| E0303 | IgnoredResult | a `Result` is discarded; use it, `?`, `match`, or `let _ =` |
| E0304 | IgnoredOption | an `Option` is discarded; use it, `?`, `match`, or `let _ =` |
| E0305 | QuestionMarkOutsideResult | `?` at script top level or in a function returning neither `Option` nor `Result` |
| E0306 | QuestionMarkTypeMismatch | `?` on an operand that is neither `Option` nor `Result` |
| E0307 | UnresolvedSumType | the sum type of a constructor is unknown |
| E0308 | UntypedSumValue | a sum value with no concrete type |
| E0309 | InvalidSumMethod | no such method on that sum type |
| E0310 | DynamicSumMethod | a sum method on a value of unknown type |
| E0311 | InvalidCollectionMethod | no such method on that collection |
| E0312 | InvalidStringMethod | no such method on `string` |
| E0313 | ModuleMemberNotPublic | the member is not `pub` |
| E0314 | SizedArrayElementNotDefaultable | `[value; N]` element has no default |
| E0315 | NegativeArraySize | an array length below zero |
| E0316 | NonConstantArrayRepeat | an array repeat count that is not constant |
| E0317 | ConstantSliceOutOfBounds | a constant slice range leaves the source |
| E0318 | MutableCollectionRequired | an index write or `push`/`pop`/`reserve` through a place whose root is immutable or a temporary |
| E0319 | ReadOnlyCollectionRequired | the operation needs a read-only receiver |
| E0320 | UnconsumedCollectionIterator | a bare `iter()` is not a value |
| E0321 | ConstantIndexOutOfBounds | a constant index leaves the collection |
| E0322 | CollectionCollectRequiresPipeline | `collect` needs a pipeline |
| E0323 | MutableCollectionAlias | a mutable alias (`let mut`, an assignment to a mutable binding, a `mut` parameter) of an aggregate reachable from an immutable binding, an imported `pub let` included |
| E0324 | NonExhaustiveStruct | a struct match needs `_` or an irrefutable field pattern |
| E0325 | GenericStructDeferred | this generic struct form is not delivered |
| E0326 | DuplicateNominal | two declarations share a name |
| E0327 | DuplicateStructField | two fields share a name |
| E0328 | UnknownStruct | no such struct |
| E0329 | InvalidStructMethod | no such method on that struct |
| E0330 | ImmutableStructField | a field write through a place whose root is immutable |
| E0331 | ImmutableStructMethod | a `mut self` method on a place whose root is immutable |
| E0332 | UnknownTrait | no such trait |
| E0333 | MissingTraitMethod | an impl omits a required method |
| E0334 | DuplicateTraitImpl | the same impl is written twice |
| E0335 | TraitMethodNotInTrait | an impl declares a method the trait does not |
| E0336 | TraitMethodSignatureMismatch | an impl method does not match the trait |
| E0337 | AmbiguousTraitMethod | more than one trait supplies that method |
| E0338 | UnsatisfiedTraitBound | a bound is not satisfied by the concrete type, or an impl promises a supertrait nothing implements |
| E0339 | OrphanTraitImpl | neither the trait nor the type is local |
| E0340 | OverlappingTraitImpl | two impls cover the same type |
| E0341 | DuplicateTraitMethod | a trait declares a method twice |
| E0342 | InvalidTraitReceiver | a receiver the trait does not allow |
| E0343 | UnresolvedGenericType | a type argument cannot be inferred |
| E0344 | RecursiveMonomorphization | instantiation recurses without decreasing |
| E0345 | MonomorphizationLimit | the instantiation limit is exceeded |
| E0346 | EnumLayoutTooLarge | an enum payload exceeds the layout limit |
| E0347 | DynamicIsNotInSurface | `dynamic` is not part of Aelys |
| E0348 | UntypedNativeValue | a native value arrives without a type |
| E0349 | UnmaterializedAppliedType | an applied type never became concrete |
| E0351 | UnboundTypeParamMethod | a method call on an unbounded type parameter |
| E0352 | UnresolvedInstanceSymbol | a generic call has no specialized instance |
| E0353 | UnresolvedTypeVariable | a type stayed open after inference |
| E0354 | PoisonedType | a type derived from an earlier error |
| E0355 | MangledSymbolCollision | two instances mangle to one symbol |
| E0356 | UnreachablePattern | an arm no value can reach |
| E0357 | GenericArityMismatch | the wrong number of type arguments |
| E0358 | PatternBindingMismatch | alternatives bind different names or types |
| E0359 | ArityMismatch | the wrong number of arguments |
| E0360 | NotCallable | the callee is not a function |
| E0361 | InfiniteType | a type would contain itself |
| E0362 | UndefinedFunction | no such function |
| E0363 | UnknownField | no such field on that type |
| E0364 | MissingField | a construction omits a field |
| E0365 | NotIterable | the value cannot be iterated |
| E0366 | InvalidIndex | the value cannot be indexed that way |
| E0367 | UntypedNativeTypeMismatch | a native signature does not match its use |
| E0368 | RecursionLimit | the type checker exceeded its recursion limit: an expression tree of 1024 nested nodes or more, so a chain of 1023 `+` is accepted and 1024 is rejected |
| E0369 | NamespaceIsNotAValue | a module path used where a value is required |
| E0370 | NotANamespace | a path segment that names no module |
| E0371 | NoSuchMember | no such member on that module |
| E0372 | UnknownTypeName | no such type is in scope |
| E0373 | InvalidTryResidual | an `Option` residual against a `Result` return, or the reverse |
| E0374 | UnsatisfiedTryConversion | no unique `From` conversion for `?` |
| E0375 | ReservedIdentityConversion | a user impl of the reserved identity `From` |
| E0376 | GlobalWithoutSignature | a global with no usable signature |
| E0377 | UndeterminedType | a type that never became determinate |
| E0378 | TypeNotImported | a public type of an imported module the selective `needs` did not name |
| E0379 | UntypedNativeBoundary | an untyped native value crosses into a typed Aelys operation |
| E0380 | TypeNestingTooDeep | a type annotation nested past the descriptor depth limit |
| E0395 | NonConstantInitializer | a `const` initializer that is not a compile-time constant, or whose evaluation divides by zero or overflows |
| E0396 | DuplicateConstant | a `const` declared twice in the same scope |
| E0390 | UndefinedLabel | a `break` or `continue` names a label no enclosing loop declares |
| E0391 | BreakValueOutsideLoop | `break` with a value targets a `while` or `for` |
| E0392 | CapitalizedBindingPattern | a bare capitalised name in a pattern on an enum, which would bind instead of matching a variant |
| E0393 | TupleFieldAssignment | a write to a tuple field; tuples are immutable |
| E0394 | ReservedName | a name starts with `AelysTuple`, which is reserved for the compiler's tuple types |

### E04xx, modules, visibility, borrows, and associated items

| Code | Name | Meaning |
|------|------|---------|
| E0401 | ModuleNotFound | no module by that path |
| E0402 | CircularDependency | modules import each other in a cycle |
| E0403 | SymbolNotPublic | the symbol is not `pub` |
| E0404 | StdlibNotAvailable | the stdlib module is not available here |
| E0405 | SymbolNotFound | no such symbol in that module |
| E0406 | InvalidNativeModule | the native module is malformed |
| E0407 | TypeNotExportable | a private nominal reaches an exported signature or an impl header |
| E0408 | NativeChecksumMismatch | the native module checksum does not match |
| E0409 | NativeVersionMismatch | the native module version does not match |
| E0410 | SymbolConflict | two declarations of one name reach one file |
| E0411 | ModulePathSeparator | a module path uses the wrong separator |
| E0412 | PrivateFieldAccess | a private field is read or written outside its owner module |
| E0413 | PrivateFieldConstruction | a private field is named in a literal or pattern outside its owner module |
| E0414 | SharedLoanMutation | mutation through a shared loan is not allowed |
| E0415 | MutableLoanOverlap | two mutable loans overlap |
| E0416 | MutableLoanAccess | a read, move, or write overlaps a live mutable loan |
| E0417 | BorrowEscapes | a call-scoped loan escapes through a binding, return, capture, or unsupported parameter |
| E0418 | BorrowInvalidated | a mutation, move, or reallocation invalidates a live loan |
| E0419 | TemporaryBorrow | a borrow target is temporary or dead |
| E0421 | MissingAssociatedItem | an impl omits a required associated type or constant |
| E0422 | AssociatedItemTypeMismatch | an associated item's declared type disagrees with the trait, or a constant's value disagrees with its own declaration |
| E0423 | AmbiguousAssociatedProjection | a projection is unresolved, ambiguous, or cyclic |
| E0424 | AssociatedBindingMismatch | a requested associated binding disagrees with the selected impl, names no item the trait declares, or binds a value where the trait declares a type and the reverse |
| E0425 | AssociatedItemOutsideTraitImpl | an impl defines an associated item the trait it implements does not declare, or an inherent impl defines one at all |
| E0426 | DuplicateAssociatedItem | an impl defines an associated item more than once |
| E0427 | AssociatedProjectionLimit | resolving a projection expands past the type-node limit |
| E0428 | UninhabitedNominalCycle | the inhabitation fixpoint fails for a type on a cycle of the constructor graph |
| E0429 | UnevaluatedAssociatedConstBinding | an associated-constant binding has a side the compiler cannot evaluate |
| E0430 | UnconstrainedImplTypeParam | an impl declares a type parameter its target type never mentions |
| E0434 | UnfoldableAssociatedConstType | an associated constant is declared, or an impl instantiates its declaration, with a type the compiler folds no constant of |
| E0435 | EmittedBytecodeRejected | the bytecode about to be written is refused by the Aelys verifier or cannot be read back, so the run writes no artifact |
| E0436 | BytecodeEncodingRefused | the program carries a name the bytecode format cannot encode, so the run writes no artifact |
| E0437 | AmbiguousTraitInstantiation | one trait, implemented for the same type at several instantiations, supplies the method a call names |
| E0438 | UnresolvedReceiverInstantiation | several impls of one trait that takes parameters, at one instantiation, each for its own instantiation of the receiver's nominal, supply the method, and the receiver is not yet a known instantiation |
| E0431 | NegativeImplOrphan | a negative impl names neither a local trait nor a local type |
| E0432 | PositiveNegativeConflict | one trait is both implemented and denied for one type |
| E0433 | OverlappingImplHeaders | two impl headers overlap without one being strictly more specific |
| E0441 | InvalidDefaultMethod | `default fn` is written outside a method of a generic trait impl |
| E0442 | UnorderedSpecialization | two impls of one trait for one type are equally specific |
| E0443 | AmbiguousSpecialization | a call lies where two incomparable impls both apply |
| E0444 | SpecializationLimit | more impls apply to one call than selection will weigh |
| E0445 | AmbiguousImport | an unqualified use of a name two whole-module imports both bind |
| E0446 | AssignToImmutableImport | an assignment to an imported `pub let` or imported function |
| E0460 | MixedIntegerOperands | a binary operator mixes two integer types |
| E0461 | WideIntegerUnavailable | `i64` or `u64` is written; `int` is a 48-bit integer |
| E0462 | FixedPointShiftOutOfRange | a literal shift count of `mul_shr` or `shl_div` lies outside 0..=63 |
| E0470 | InvalidMapKey | a `Map` key type other than an integer, `string`, `char` or `bool` |
| E0471 | UnorderedSortElement | `sort` on elements with no built-in order |
