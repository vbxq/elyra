# Getting Started

This guide walks you through writing your first Elyra programs. I'll assume you've already [installed](installation.md) the compiler.

## Hello World

Create a file called `hello.elyra`:

```rust
println("Hello, World!")
```

Run it:

```bash
elyra hello.elyra
```

That's your first program ! The safe standard modules (`std::io`, `std::math`, `std::string`, `std::time`) are auto-registered at startup, so you don't need to import them. The other modules (`std::fs`, `std::net`, `std::sys`) need a `use` line, such as `use std::fs`.

You don't need a `main` function, top-level code just runs.

## Variables

```rust
let name = "Reimu"
let age = 30
let pi = 3.14159
```

Variables are immutable by default. If you need to change a value, use `mut`:

```rust
let mut counter = 0
counter++
counter++
println(counter)  // prints: 2
```

## Type Annotations

Types are optional. You can add them when you want clarity or when the compiler can't figure it out:

```rust
let x: int = 42
let pi: float = 3.14159
let name: string = "Bob"
let active: bool = true
```

Available types include `int`, `float`, `string`, `bool`, `Option<T>`,
`Result<T, E>`, fixed arrays `[T; N]`, and growable `Vec<T>`. Absence uses
`Option<T>`; there is no `null` value in the surface language.

The type system uses Hindley-Milner inference: every value has a static type, whether you write it or the compiler works it out. When nothing in the program pins a type down, compilation stops with an error asking for an annotation (E0353); there is no dynamic fallback.

## Functions

```rust
fn greet(name: string) {
    println("Hello, {name}!")
}

greet("World")
```

Return values:

```rust
fn add(a: int, b: int) -> int {
    return a + b
}

// or implicitly (last expression is returned)
fn multiply(a: int, b: int) -> int {
    a * b
}
```

You can omit types entirely if you prefer:

```rust
fn add(a, b) {
    return a + b
}

println(add(2, 3))  // prints: 5
```

The parameter types then come from the call sites, so a function like this that is never called is an error (E0353). I recommend adding types at the function signature !

## Control Flow

### if/else

```rust
let x = 10

if (x > 5) {                           // parentheses optional
    println("big")
} else if x > 0 {
    println("small")
} else {
    println("zero or negative")
}
```

No need to put parentheses around the condition, braces are mandatory though

### while

```rust
let mut i = 0
while i < 5 {
    println(i)
    i++
}
```

### for

For loops use ranges:

```rust
// 0 to 9 (exclusive end)
for i in 0..10 {
    println(i)
}

// 1 to 10 (inclusive end)
for i in 1..=10 {
    println(i)
}

// with step
for i in 0..100 step 10 {
    println(i)  // 0, 10, 20, ..., 90
}
```

`break` and `continue` work as expected:

```rust
for i in 0..100 {
    if i == 50 { break }
    if i % 2 == 0 { continue }
    println(i)  // odd numbers only, up to 49
}
```

You can iterate directly over arrays and vectors:

```rust
let numbers = [10, 20, 30]
for item in numbers {
    println(item)
}

let names = vec!["Alice", "Bob", "Charlie"]
for name in names {
    println(name)
}
```

You can also iterate over a string's characters:

```rust
for letter in "hello" {
    println(letter)
}
```

This works with variables too:

```rust
let name = "Marisa"
for c in name {
    println(c)
}
```

## Logical Operators

They are `&&`, `||` and `!`, and `&&` and `||` short-circuit (the words `and`, `or` and `not` are not operators):

```rust
let x = 3
let y = 4
let valid = x < y

if x > 0 && y > 0 {
    println("both positive")
}

if !valid {
    println("invalid")
}
```

## Strings

Strings support the usual escape sequences (`\n`, `\t`, `\\`, `\"`):

```rust
println("Line 1\nLine 2")
println("Tab\there")
```

You can access individual characters with `[]`:

```rust
let s = "hello"
println(s[0])  // prints: h
println(s[4])  // prints: o
```

Each index returns a `char`, the same type a for-each over a string yields. Indexing is Unicode-aware (it counts characters, not bytes).

Concatenation uses `+`:

```rust
let name = "Marisa"
let greeting = "Hello, " + name + "!"
println(greeting)  // prints: Hello, Marisa!
```

Or use interpolation with `{expression}`:

```rust
let name = "Quar"
println("Hello, {name}!")        // prints: Hello, Quar!
println("2500 + 2500 = {2500 + 2500}")       // prints: 2500 + 2500 = 5000
```

Double braces for literal braces: `"{{key}}"` gives `{key}`

## Arrays and Vectors

### Arrays

Arrays hold multiple values of the same type:

```rust
let numbers = [10, 20, 30, 40, 50]
println(numbers[0])  // prints: 10
println(numbers[2])  // prints: 30
println("{numbers.len()}")  // prints: 5
```

Modify elements like this:

```rust
let mut scores = [95, 87, 92]
scores[1] = 90
println(scores[1])  // prints: 90
```

You can add type annotations if you want to be explicit:

```rust
let ints: [int; 3] = [1, 2, 3]
let floats: Vec<float> = vec![1.5, 2.7, 3.9]
```

Empty arrays need a type:

```rust
let empty: [int; 0] = []
```

If you need an array of a specific size without listing each element:

```rust
let zeros = [0; 10]           // fixed-size array
let buffer = vec![0; 100]     // growable vector
```

### Vectors

Vectors can grow after you create them:

```rust
let mut v = vec![1, 2, 3]
v.push(4)
v.push(5)
println("{v.len()}")  // prints: 5

let last = v.pop()
println("{last}")  // prints: Some(5)
```

`pop` returns an `Option`: `Some(value)`, or `None` when the vector is empty.

Good for building lists on the fly:

```rust
let mut scores = vec![]
scores.push(95)
scores.push(87)
scores.push(92)

let mut sum = 0
for i in 0..scores.len() {
    sum += scores[i]
}
println("{sum / scores.len()}")  // prints: 91
```

### Sharing and copying

Arrays, vectors, maps and structs are shared by reference: `let b = a` gives a second name to the same array, and a change through one name shows through the other. `clone()` makes an independent copy:

```rust
let mut a = [1, 2, 3]
let mut b = a          // b and a are the same array
b[0] = 10
println(a[0])          // prints: 10

let mut c = a.clone()  // an independent copy
c[0] = 99
println(a[0])          // prints: 10
```

Sharing a mutable name for a value that an immutable `let` holds is refused (error E0323): write `let mut b = a.clone()` for a copy, or declare `a` with `let mut` to share it.

### When to use which

- **Array**: Size is fixed (coordinates, color values)
- **Vec**: Size changes (user input, dynamic lists)

## A Complete Example

Here's a program that calculates factorials:

```rust
fn factorial(n: int) -> int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

println("Factorials:")

for i in 1..=10 {
    let result = factorial(i)
    println("{i}! = {result}")
}
```

## Editor Support

Writing Elyra code is way more pleasant with syntax highlighting.
Many other editor coming, don't worry !

### VSCode Extension

Someone made a VSCode Extension for Elyra, see [Acknowledgements](../ACKNOWLEDGEMENTS.md)
Download [Aelys Language Support](https://marketplace.visualstudio.com/items?itemName=SpaceGame.aelys-lang) from the VSCode marketplace, or directly to the [repository link](https://github.com/SpaceGame-wq/aelys-vscode)

## What's Next

- [Language Specification](language-spec.md) - complete syntax reference
- [Standard Library](standard-library.md) - available modules and functions
- [Examples](../examples/README.md) - array examples, demos, and more
- [Performance](performance-benchmarks.md) - benchmarks and optimization tips
