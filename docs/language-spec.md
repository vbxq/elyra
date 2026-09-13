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

### Reserved Words

```
let mut fn if else while for in step return break continue match struct enum
trait impl and or not pub needs as from true false null
```

`null` is reserved only so that it can be rejected; see [Booleans](#literals) below.

`where`, `dyn`, and `self` are contextual. They carry meaning in a generic
bound, a type position, and a method receiver respectively, and they are still
ordinary identifiers everywhere else:

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

Integers are 48-bit signed (roughly +-140 trillion). This is due to NaN-boxing, the VM packs type information into the unused bits of IEEE 754 NaN values. You'll probably never hit this limit in practice.

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

UTF-8 encoded. Escape sequences: `\n` (newline), `\t` (tab), `\r` (carriage return), `\\` (backslash), `\"` (quote).

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
| `int` | Signed integer | 48-bit |
| `float` | Floating point | 64-bit |
| `string` | UTF-8 text | heap allocated |
| `bool` | Boolean | 1 bit (packed) |
| `unit` | No meaningful value | immediate |
| `Option<T>` | A value or absence | heap allocated when needed |
| `Result<T, E>` | Success or failure | heap allocated when needed |
| `Error` | Structured error value | heap allocated when needed |
| `fn(T, ...) -> R` | Function/closure | heap allocated |
| `Name { field: T, ... }` | Fixed-layout struct | heap allocated |
| `[T; N]` | Fixed-size array | heap allocated |
| `Vec<T>` | Growable vector | heap allocated |

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
parameter names. `function` is not a type: `let f: function`, `fn f(g: function)`
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

Mutable parameters are value copies. Modifying them inside the function doesn't affect the caller:

```rust
fn try_modify(mut x: int) -> int {
    x += 100
    return x
}

let a = 1
let b = try_modify(a)  // b = 101
a                       // still 1
```

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

Integer division truncates: `7 / 2` gives `3`. Use floats if you need decimal results.

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

These are postfix operators and work on identifiers only.

### Comparison

| Operator | Description |
|----------|-------------|
| `==` | Equal |
| `!=` | Not equal |
| `<` | Less than |
| `<=` | Less than or equal |
| `>` | Greater than |
| `>=` | Greater than or equal |

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
| `>>` | Right shift (arithmetic) |

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

### while

```rust
let mut n = 0
while n < 3 {
    n++
}
println(n)   // 3
```

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

String iteration is Unicode-aware, so each `c` is a single-character string, not a byte; so it means that multi-byte characters like `é` or `😀` are handled well !

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
    if c == "d" { break }
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

Work in `while` loops too.

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

### Standard Library Modules

The safe standard library modules are **auto-registered**, their functions are available without any `needs` statement:

- `std::io` - console I/O
- `std::math` - math functions and constants
- `std::string` - string manipulation
- `std::convert` - type conversions
- `std::time` - time and timers


See [Standard Library](standard-library.md) for full documentation.

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

Top-level definitions in a file become the module's exports.

A top-level `let` is a variable of the module that declares it, and it has one
slot per module. Every reference inside that module resolves to that slot: the
module's own top-level code, its free functions, and the bodies of its impls,
whether such a body runs in the module or in a file that imported it. Two modules
that each declare `v` hold two variables, and a file that imports one of them and
also declares its own `v` holds a third. An importer's binding of an imported
`pub let mut` holds the value the module had when it was imported; assigning to
that binding moves the importer's binding and not the module's variable.

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
path or through a module alias, the module's own globals, public or not, and the
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
**generic** private declaration that reaches the import contract has nothing
left to send, monomorphization having erased it inside its own module, so a body
that names one there is E0407, the refusal its public twin already gets; a
body-only generic travels as a template and monomorphizes in the importer.

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
`N` is an integer literal, an arithmetic expression over such literals, or an
associated constant named on a concrete type or on `Self`, the same lengths the
annotation `[T; N]` accepts. `vec![value; count]` creates a Vec and permits a
dynamic count. The element is
evaluated once and copied into each slot. There is no runtime-sized array
constructor; use a Vec when the count is not known at compile time.

```rust
let zeros = [0; 10]
let repeated = [7; 3]
let mut buffer = vec![0; 4]
```

All elements must have one common type. A fixed array annotation is checked at
compile time, including its length:

```rust
let values: [int; 3] = [1, 2, 3]
let mut numbers: Vec<int> = vec![1, 2]
numbers.push(3)
numbers[0] = 9
```

Mutation requires `let mut`, a mutable parameter, or a mutable base binding
for an indexed place. `push`, `pop`, and `reserve` are Vec operations. `pop`
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
length is a compile error. Other invalid indices trap at runtime; they never
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

### String Indexing

Strings support character-based indexing with `[]`:

```rust
let s = "hello"
s[0]    // "h"
s[1]    // "e"
s[4]    // "o"
```

Each index returns a single-character string. Indexing is Unicode-aware: it accesses the Nth character, not the Nth byte.

You can combine string indexing with a range-based for loop:

```rust
let s = "hello"
for i in 0..s.len() {
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

Matching an enum is exhaustive at compile time. Guards do not cover a
constructor, and nested patterns are checked recursively. A missing arm emits
E0302 `non-exhaustive match; missing E::B`, naming the missing constructor, with
the help line `add a missing arm or '_'`. Alternation may combine constructors
when all alternatives bind the same names and types. Integer and string matches
still require `_`.

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
offset, not looked up by a runtime string key. A field write requires a mutable
root binding; an immutable binding or a temporary cannot be used as the root.

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
the **bare name** of the target type, and the method. The trait's own type
arguments follow in the same encoding, rendered with the impl's type parameters
spelled as the impl wrote them, and a generic impl then carries one suffix per
instance holding the substituted target type. The target type's own type
arguments never enter the three names, so `impl Source for Wrap<int>` mangles
`next` to `__aelys_trait::00000006:Source00000004:Wrap00000004:next`. A type
parameter the target type never mentions therefore leaves no trace in the
symbol. Two impls of one trait for two instantiations of one constructor
therefore write one slot, so they are refused with E0355 as the second impl
registers its method, the caret on that method, or on the impl's target type
when the method is a default body adopted from the trait. The check compares the
substituted target type held against the symbol, so one impl inlined into
several importers stays equal to itself; a generic impl carries no target
arguments in the symbol at all and is left to the monomorphizer, which raises
the same code on the per-instance suffix. Renaming the method in one impl, or
declaring it in a second trait, separates the two slots.

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
match it, call a consuming method, or write `let _ = expression` explicitly.

The `?` operator propagates `Err` or `None` from a compatible enclosing return type.
The available consuming methods include `unwrap`, `expect`, `unwrap_or`,
`unwrap_or_else`, `ok`, `err`, `map`, `map_err`, `and_then`, and `or_else`.
There is no null literal or null-inspection builtin in the surface language.
The closed built-in `Error` family has one data-carrying constructor,
`Error::Message(string)`.

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
`Result`, `T` must unify with the value context, and either `E` equals `F` or
exactly one `From<E> for F` impl is selected. Selection tries the identity rule
first and then the non-identity impls; the failure path calls the selected `from`
symbol directly and returns `Err`. It never stringifies the error and never calls a
runtime converter.

`Option<T>?` is valid only in an `Option<U>` context and propagates `None`. Mixing
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

### E01xx, syntax

| Code | Name | Meaning |
|------|------|---------|
| E0101 | UnexpectedToken | a token the grammar does not allow here |
| E0102 | ExpectedExpression | an expression was required |
| E0103 | ExpectedIdentifier | an identifier was required |
| E0104 | InvalidAssignmentTarget | the left side of `=` is not a place |
| E0105 | RecursionDepthExceeded | the parser exceeded its nesting limit |
| E0106 | NullIsNotInSurface | `null` is not part of Aelys |
| E0107 | ExpectedPattern | a pattern was required |
| E0108 | InvalidPattern | a pattern the grammar does not allow |
| E0109 | UnknownVariant | no such variant on that type |
| E0110 | MatchArmValueRequired | a match arm must produce a value |
| E0111 | BorrowingReceiverDeferred | `&self` and `&mut self` are Stage 3 |
| E0113 | TraitObjectDeferred | `dyn Trait` is Stage 3 |

### E02xx, names and emission limits

| Code | Name | Meaning |
|------|------|---------|
| E0201 | UndefinedVariable | no such name in scope |
| E0202 | VariableAlreadyDefined | a name is declared twice in one scope |
| E0203 | AssignToImmutable | assignment to a binding without `mut` |
| E0204 | TooManyConstants | the constant pool limit is exceeded |
| E0205 | TooManyRegisters | the register limit is exceeded |
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

### E03xx, types

| Code | Name | Meaning |
|------|------|---------|
| E0301 | TypeInferenceError | a type mismatch or an unresolved name |
| E0302 | NonExhaustiveMatch | a match is missing a constructor or `_` |
| E0303 | IgnoredResult | a `Result` is discarded |
| E0304 | IgnoredOption | an `Option` is discarded |
| E0305 | QuestionMarkOutsideResult | `?` outside a compatible return type |
| E0306 | QuestionMarkTypeMismatch | `?` on an operand of the wrong type |
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
| E0318 | MutableCollectionRequired | the operation needs a mutable receiver |
| E0319 | ReadOnlyCollectionRequired | the operation needs a read-only receiver |
| E0320 | UnconsumedCollectionIterator | a bare `iter()` is not a value |
| E0321 | ConstantIndexOutOfBounds | a constant index leaves the collection |
| E0322 | CollectionCollectRequiresPipeline | `collect` needs a pipeline |
| E0323 | MutableCollectionAlias | a mutable collection is aliased |
| E0324 | NonExhaustiveStruct | a struct match needs `_` or an irrefutable field pattern |
| E0325 | GenericStructDeferred | this generic struct form is not delivered |
| E0326 | DuplicateNominal | two declarations share a name |
| E0327 | DuplicateStructField | two fields share a name |
| E0328 | UnknownStruct | no such struct |
| E0329 | InvalidStructMethod | no such method on that struct |
| E0330 | ImmutableStructField | a field write through an immutable root |
| E0331 | ImmutableStructMethod | a `mut self` method on an immutable value |
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
| E0368 | RecursionLimit | the type checker exceeded its recursion limit |
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

### E04xx, modules, visibility, borrows, and associated items

| Code | Name | Meaning |
|------|------|---------|
| E0401 | ModuleNotFound | no module by that path |
| E0402 | CircularDependency | modules import each other in a cycle |
| E0403 | SymbolNotPublic | the symbol is not `pub` |
| E0404 | StdlibNotAvailable | the stdlib module is not available here |
| E0405 | SymbolNotFound | no such symbol in that module |
| E0406 | InvalidNativeModule | the native module is malformed |
| E0407 | TypeNotExportable | a nominal value cannot cross a module boundary |
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
| E0431 | NegativeImplOrphan | a negative impl names neither a local trait nor a local type |
| E0432 | PositiveNegativeConflict | one trait is both implemented and denied for one type |
| E0433 | OverlappingImplHeaders | two impl headers overlap without one being strictly more specific |
| E0441 | InvalidDefaultMethod | `default fn` is written outside a method of a generic trait impl |
| E0442 | UnorderedSpecialization | two impls of one trait for one type are equally specific |
| E0443 | AmbiguousSpecialization | a call lies where two incomparable impls both apply |
| E0444 | SpecializationLimit | more impls apply to one call than selection will weigh |
