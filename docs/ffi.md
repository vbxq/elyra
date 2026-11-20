# Calling C libraries

An `extern` block declares functions of a C shared library (`.so`, `.dylib`,
`.dll`) that Elyra code then calls like its own functions. No wrapper module
is needed: drop the library next to the script, or install it system-wide, and
declare the functions you use.

```rust
extern "c" {
    fn strlen(s: cstr) -> u64
    fn abs(x: i32) -> i32
}
extern "m" {
    fn cos(x: float) -> float
    fn powf(x: f32, y: f32) -> f32
}

println(strlen("hello"))     // prints: 5
let n = -42
println(abs(n))              // prints: 42
println(cos(0.0))            // prints: 1.0
println(powf(2.0, 3.0))      // prints: 8.0
```

The language reference for the construct is the [C FFI](language-spec.md#c-ffi)
section of the specification. This page is the practical guide.

## Declaring functions

```
extern "<library>" {
    fn <name>(<param>: <type>, ...) -> <type>
    fn <name>(<param>: <type>, ...) -> <type>
}
```

- The block sits at the top level of a file (E0492 elsewhere). A file may
  hold any number of blocks, for one library or several.
- Each `fn` names a C symbol; the Elyra function has the same name. A name
  starting with `__` is reserved, as everywhere in Elyra.
- A declaration is hoisted like a function: code above the block, and
  functions of the same file, may call it.
- An extern function is a value of its function type: `let f = abs`,
  `apply(abs, -3)` with `apply(h: fn(i32) -> i32, ...)`.
- `pub` is refused (E0496): an extern function belongs to the file that
  declares it. A module that wraps a library exports `pub fn` wrappers.
- The name is private to its file. Two modules, or a module and the main
  script, may declare the same name, from the same library or from different
  ones, and an `fn` or `let` of that name in another file never replaces it:
  each file's code, its generic functions included, calls its own declaration.
- Two declarations of one name, or an extern function and an `fn` or a
  top-level `let` of the same name in one file, are E0495.
- `extern` is contextual: it starts a block only before a string literal.

## Types

| Written | C type | Accepted argument | Result in Elyra |
|---|---|---|---|
| `i8` `i16` `i32` `i64` | `int8_t` ... `int64_t` | any integer | `i8` ... `i64` |
| `u8` `u16` `u32` `u64` | `uint8_t` ... `uint64_t` | any integer | `u8` ... `u64` |
| `int` | `int64_t` | any integer | `int` |
| `f32` | `float` | `float` or `f32` | `f32` |
| `float`, `f64` | `double` | `float` or `f32` | `float` |
| `bool` | `bool` / `_Bool` (one byte) | `bool` | `bool` |
| `ptr` | any pointer (`void *`, `T *`, a function pointer) | `ptr` | `ptr` |
| `cstr` | `const char *` | `string` | `string` |
| `&[u8]` | `const uint8_t *` | `&v` with `v: Vec<u8>` | not a result |
| `&mut [u8]` | `uint8_t *` | `&mut v` with `v: Vec<u8>` | not a result |
| no `->` | `void` | | `unit` |

The types name the C side exactly: C `int` is `i32`, `long` is `i64` on
Linux and macOS and `i32` on Windows, `size_t` is `u64` on a 64-bit target,
`char` is `i8` or `u8`. A C function that returns an `int` used as a boolean
is declared `-> i32`.

### Integers

An integer parameter takes an argument of any integer type and converts it
exactly. A value outside the C type's range is never truncated: the call
stops with a runtime error naming the function, the argument and the type,
`C function 'abs': argument 1: 3000000000 does not fit in i32`. A literal is
checked when the program compiles instead (`abs(3000000000)` is E0301). An
integer result keeps every bit of the C value.

### Floats

`f32` arguments are rounded to the nearest single-precision value, as C does.
A returned `f32` is the exact single-precision value, an Elyra `f32`.

An Elyra `f32` is stored as a double and is not rounded when written in Elyra:
`sqrtf(0.01)` prints `0.10000000149011612`, the single-precision value C
returns, and compares unequal to `0.1 as f32`, which keeps every digit of the
double `0.1`. Compare results of C single-precision functions with a
tolerance.

### Pointers: `ptr`

`ptr` is an opaque address. It has no arithmetic and no conversion with `as`;
it can be compared with `==` and `!=`, stored in a variable, a struct field, a
`Vec` or an `Option`, used as a `Map` key (compared by address), and passed
back to C. Printing a `ptr` shows its address in hexadecimal and never reads
the memory it points to: `println(p)`, `"{p}"` and `p.to_string()` all give
`0x1000` for `ptr::from_addr(4096)`. A collection holding `ptr` values has no
`Display` (E0338): print its elements.

| Operation | Result |
|---|---|
| `ptr::null()` | the null pointer |
| `p.is_null()` | `bool` |
| `p.offset(n)` | `p` moved by `n` bytes (`n` of any integer type, negative included) |
| `p.addr()` | the address as a `u64` |
| `p.to_string()` | the address in hexadecimal, `"0x1000"` |
| `ptr::from_addr(n)` | the pointer at address `n` (0 to `u64` max) |
| `p.read_<t>(off)` | the value of type `t` at `p + off`, for `t` in `i8` `i16` `i32` `i64` `u8` `u16` `u32` `u64` `int` (a `float` for `f32` and `f64`, a `ptr` for `ptr`) |
| `p.write_<t>(off, x)` | stores `x` at `p + off`; an integer outside the range of `t` is a runtime error |
| `ptr::alloc(n)` | `n` zeroed bytes from the C allocator (`calloc`) |
| `ptr::free(p)` | releases memory from `ptr::alloc` or from C's `malloc` |
| `ptr::to_string(p)` | copies the NUL-terminated UTF-8 string at `p` into a `string` (`p.to_string()` is the address, not this) |

A read or write through the null pointer, and an offset that leaves the
address space, are runtime errors. Reads and writes use the host byte order
and need no alignment. A binding named `ptr` hides the `ptr::` functions in
its file.

```rust
extern "c" {
    fn malloc(n: u64) -> ptr
    fn free(p: ptr)
}
let p = malloc(16)
p.write_i32(0, -7)
p.write_f64(8, 2.5)
println(p.read_i32(0))       // prints: -7
println(p.read_f64(8))       // prints: 2.5
println(p.offset(8).read_f64(0))   // prints: 2.5
free(p)
println(ptr::from_addr(4096))      // prints: 0x1000
```

### Strings: `cstr`

A `cstr` argument is a NUL-terminated copy of the Elyra string, valid until the
C function returns. A string holding a NUL byte cannot cross and stops the
call with a runtime error.

A `cstr` result is copied into a new Elyra string; the C memory is not freed.
The bytes must be valid UTF-8 and the pointer non-null, or the call stops with
a runtime error (`returned a string that is not valid UTF-8`, `returned a
null cstr`): Elyra never builds a lossy or truncated string. When C may return
NULL, or returns memory the caller must free, declare the result `ptr`, test
`is_null()`, read it with `ptr::to_string(p)` and free it yourself.

```rust
extern "c" {
    fn getenv(name: cstr) -> ptr
}
let home = getenv("HOME")
if !home.is_null() { println(ptr::to_string(home).len() > 0) }   // prints: true
println(getenv("AELYS_UNSET_EXAMPLE_VARIABLE").is_null())        // prints: true
```

### Byte buffers: `&[u8]` and `&mut [u8]`

A byte buffer parameter receives a pointer to the `Vec<u8>`'s own packed
storage, for the duration of the call: nothing is copied, and C's writes
through a `&mut [u8]` are visible to Elyra after the call. The borrow rules
are those of native modules: `&mut v` needs a mutable `v` (E0415) and cannot
sit next to another borrow of `v` in the same call (E0415, E0416), and the
length is not passed: give it to C yourself. Called through a function value
(`let f = memset`), the function takes the `Vec<u8>` itself, `f(v, 0, 4)`, not
`&mut v` (E0417); the compiler cannot see that call, so lending the same Vec
mutably twice in it is a runtime error instead.

```rust
extern "c" {
    fn memset(b: &mut [u8], c: i32, n: u64) -> ptr
    fn memcpy(dst: &mut [u8], src: &[u8], n: u64) -> ptr
}
let mut a: Vec<u8> = vec![0; 4]
memset(&mut a, 7, 4)
let mut b: Vec<u8> = vec![0; 4]
memcpy(&mut b, &a, 4)
println(b)                   // prints: [7, 7, 7, 7]
```

A C structure is laid out in bytes: allocate it with `ptr::alloc(size)` or keep
it in a `Vec<u8>`, fill and read it with the `read_*`/`write_*` methods (or
the `Vec<u8>` ones, `v.write_i32_le(off, x)`), and pass the pointer. Copy C
memory into a `Vec<u8>` with `memcpy` from libc, declared with a `ptr`
source.

## Variadic functions

A parameter list ending in `...` declares a variadic C function. The values
after the fixed parameters are passed with C's default promotions:

| Argument type | Passed as |
|---|---|
| `i8` `i16` `i32` `u8` `u16` `bool` | `int` |
| `u32` | `unsigned int` |
| `int`, `i64` | `long long` |
| `u64` | `unsigned long long` |
| `float`, `f32` | `double` |
| `ptr` | a pointer |
| `string` | a NUL-terminated copy (`char *`) |

Any other type is E0494, and so is a borrow such as `&mut buf`: a byte buffer
crosses only as a declared `&[u8]` or `&mut [u8]` parameter, so for `sscanf`'s
output arguments pass a `ptr` from `ptr::alloc(n)` and read it back with
`read_i32` and the other `read_*` methods. An `int` is 64 bits, so `printf`'s
`%d` wants `x as i32` and `%lld` takes an `int` directly. A variadic function
can only be called by name (E0493 when it is used as a value).

C's `printf` writes through the C library's own `stdout` buffer, which is
separate from the one behind `println`. When the output is not a terminal, C
flushes its buffer only when it is full or the process exits, so `printf` text
can appear after later `println` lines. Declare `fn fflush(f: ptr) -> i32` and
call `fflush(ptr::null())` after the C output to keep the order.

```rust
extern "c" {
    fn snprintf(buf: &mut [u8], n: u64, fmt: cstr, ...) -> i32
}
let mut buf: Vec<u8> = vec![0; 32]
let len = snprintf(&mut buf, 32, "%s %lld %.1f", "n", 42, 0.5)
buf.truncate(len as int)
println(buf.to_utf8().unwrap())    // prints: n 42 0.5
```

Variadic calls work on x86-64 Linux and macOS and on 64-bit ARM Linux. On
other targets (Windows, Apple silicon) the first call of a variadic function
is a runtime error; declare a fixed-arity C helper instead.

## Finding the library

The library string is a bare name (`"SDL2"`, `"GL"`, `"vulkan"`, `"c"`, `"m"`)
or a file name or path. A name that contains a `/` or `\`, or ends in `.so`,
`.dylib`, `.dll` or contains `.so.`, is a file: a relative path is looked up
next to the script first, then handed to the system loader as written. A bare
name is looked up in this order:

1. the path a manifest gives it, `main.elyra.toml` or `elyra.toml` next to the
   script:

   ```toml
   [library.SDL2]
   path = "lib/libSDL2-2.0.so.0"
   ```

2. next to the declaring file and next to the script: `lib<name>.so` and
   `<name>.so` on Linux, `lib<name>.dylib` and `<name>.dylib` on macOS,
   `<name>.dll` and `lib<name>.dll` on Windows;
3. the system loader with `lib<name>.so` (`.dylib`, `.dll`), then on macOS
   Homebrew's directories and `<name>.framework`, and on Linux every
   versioned `lib<name>.so.N` in `LD_LIBRARY_PATH` and the standard library
   directories, newest version first (so `"c"` finds `libc.so.6`).

The library is opened and the symbol looked up at the first call of each
declared function, not when the program starts: a program can declare
functions an older library lacks and only fail when it calls one. A missing
library is a runtime error that names the function and lists every file tried,
`cannot load C library 'SDL3' for C function 'SDL_Init': tried libSDL3.so; last
error: ...`, and a
missing symbol names the symbol and the file, `C library 'c' (/lib/libc.so.6)
has no symbol 'SDL_Init'`. A loaded library stays loaded until the process
exits.

## Calls, threads and the JIT

Each call converts the arguments, then jumps through a small trampoline that
Cranelift generates once per C signature for the host's C calling convention,
and converts the result. Calls run on the VM's thread; the CLI runs the VM on
the main thread, which SDL video on macOS requires. Machine code from the JIT
calls an extern function through the same path as any native function, never
inline.

C code cannot call back into Elyra: a function-typed parameter is E0491.
Structures are never passed by value (E0490): pass a pointer to their bytes.

## What the FFI does not protect against

The compiler checks Elyra's side of every call; it cannot see the C side. A
wrong declaration or a wrong pointer is undefined behaviour and usually ends
the process:

- a declaration that does not match the C prototype (wrong types, missing or
  extra parameters, a wrong result type, `int` for a C `int`);
- a dangling, freed, foreign or misaligned-for-C pointer, a double `free`,
  freeing memory the C library owns, or memory leaks: `ptr` is never tracked;
- a length larger than the buffer handed to C (`memset(&mut v, 0, 100)` on a
  4-byte Vec writes past it);
- `ptr::to_string(p)` on memory that holds no NUL byte: it reads on past the
  end until it meets one;
- C keeping a pointer to a `cstr` argument or a byte buffer after the call:
  both are valid only during the call, and a Vec may move when it grows;
- C threads, signal handlers, `longjmp` across Elyra frames, `errno`
  (read it through the C function your platform provides, such as
  `__errno_location`, wrapped in a C helper since `__` names are reserved).

## Performance

An extern call costs about as much as a native module call. Measured on a
release build with 10 million calls in a loop, `labs` from libc takes about
660 instructions per call and a native module function computing the same
value about 600, over a loop iteration of about 200 instructions. Most of
either cost is the VM's native call path, not the C call. Keep per-pixel and
per-sample loops in Elyra and cross into C once per frame or per batch.

## Example: an SDL2 window

`examples/ffi/sdl_window.elyra` opens a window, fills a `Vec<u8>` with a
gradient each frame and shows it through an SDL streaming texture, with no
wrapper module, only the system libSDL2. `examples/ffi/gl_clear.elyra`
creates an OpenGL context with SDL and clears it with `glClearColor` and
`glClear`; under a virtual X display it runs on Mesa's llvmpipe.

```sh
elyra run examples/ffi/sdl_window.elyra 120      # frames to show
elyra run examples/ffi/gl_clear.elyra 60
```
