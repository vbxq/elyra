# Native modules

A native module is a shared library (`.so`, `.dylib`, `.dll`) written against
the aelys-native ABI, usually in Rust with the `aelys-native` crate. Elyra
loads it with `use`, type-checks every call against the signatures the
module declares, and calls it on the VM thread.

To call a plain C library (SDL2, OpenGL, zlib, libc) no module is needed:
declare its functions in an `extern` block, see [Calling C libraries](ffi.md).
A native module is for a typed Rust API: results such as `Option` and
`Result`, strings handed back without a C allocation, state kept across calls.
`modules/sdl` is the reference example.

## Writing a module

```toml
# Cargo.toml
[package]
name = "my-module"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
aelys-native = { path = "<elyra checkout>/native" }

[workspace]
```

```rust,ignore
use aelys_native::aelys_module;

#[aelys_module(name = "pixels", version = "0.1.0")]
mod exports {
    use aelys_native::CallContext;

    #[aelys_export]
    pub fn checksum(frame: &[u8]) -> i64 {
        frame.iter().map(|&byte| i64::from(byte)).sum()
    }

    #[aelys_export]
    pub fn clear(frame: &mut [u8], value: i64) {
        frame.fill(value as u8);
    }

    #[aelys_export]
    pub fn describe(context: &mut CallContext<'_>, frame: &[u8], label: String) -> String {
        let _ = context;
        format!("{label}: {} bytes", frame.len())
    }

    #[aelys_export]
    pub fn parse(text: String) -> Result<i64, String> {
        text.trim().parse().map_err(|_| format!("not a number: {text}"))
    }
}
```

`#[aelys_module]` generates the C wrappers, the export table, the signatures
and the module descriptor; no descriptor is written by hand. The `name` is the
name Elyra code uses (`use pixels`, `pixels::checksum(&frame)`), and it must also
be the file name the library is copied to (`pixels.so`).

### Types that cross the boundary

| Rust parameter | Elyra argument | notes |
|---|---|---|
| `i64` | `int` | the whole i64 range |
| `f64` | `float` | |
| `bool` | `bool` | |
| `String` | `string` | copied into the native |
| `&[u8]` | `&v` with `v: Vec<u8>` | read-only view of the whole Vec |
| `&mut [u8]` | `&mut v` with `v: Vec<u8>` | writable view; writes are visible to Elyra after the call |
| `&mut CallContext<'_>` | nothing | not an Elyra argument; at most one per function |

| Rust result | Elyra type |
|---|---|
| `i64`, `f64`, `bool`, `String`, `()` | `int`, `float`, `bool`, `string`, unit |
| `Option<T>` (T one of the above) | `Option<T>` |
| `Result<T, String>` | `Result<T, string>` |

A byte parameter borrows a `Vec<u8>` for the duration of the call, with Rust's
spelling and rules, checked by the compiler before the call runs:

- `&[u8]` takes `&v`; `&mut [u8]` takes `&mut v`, and `v` must be a mutable
  place (`let mut v`, a `mut` parameter, or a field or element of one). Passing
  `v` without `&` is E0417, `&` of a temporary such as `&s.to_bytes()` is
  E0419, and a `Vec<int>` or a string is E0301.
- One call may take the same Vec through several `&` arguments
  (`sdl::present_indexed(&px, &px)`), but `&mut v` next to any other borrow of
  `v` in the same call (`pixels::copy_into(&mut v, &v)`) is refused at compile
  time.

<!-- doctest: ignore (needs the pixels native module built from the code above) -->
```rust
use pixels
let mut frame: Vec<u8> = vec![0; 64000]
pixels::clear(&mut frame, 7)
let sum = pixels::checksum(&frame)
```

Structs, arrays, other `Vec` types, maps, closures and raw pointers do not
cross. An `int` crosses as an `i64`: the generated wrapper reads a value in
-2^47..2^47 from the NaN-boxed argument itself and a larger one, which the VM
keeps on its heap, through the `read_int` slot of the VM API; a result outside
-2^47..2^47 is allocated through the `alloc_int` slot (API version 5). A `u64`
above `9223372036854775807` is not an `int` and does not cross as one.

A `Vec<u8>` stores one byte per element, so a byte parameter is the Vec's own
storage, not a copy: borrowing costs the same for a 16-byte header and a 28 MB
file, and a write through `&mut [u8]` lands in the Vec directly. The slice is
valid only during the call; the Vec cannot grow or shrink while a native holds
it, because no Elyra code runs until the native returns.

### The call context

`CallContext<'call>` is the native side of one call:

- `alloc_string(&str) -> Result<AelysValue, i32>` creates an Elyra string,
  which stays alive until the native function returns, however many values
  the call allocates after it (the raw `alloc_string_from_context` behaves the
  same way);
- `read_string(AelysValue) -> Option<String>` copies an Elyra string out;
- `read_int(AelysValue) -> Option<i64>` reads an `int` argument, inline or boxed
  (the raw `read_int_from_value`; `value_as_int` alone only reads an inline one
  and gives `0` for a boxed value);
- `int(i64) -> Result<AelysValue, i32>` makes an `int` result of any size (the
  raw `alloc_int_from_context`), alive until the native function returns;
- `borrow_bytes(value) -> Result<&'call mut [u8], i32>` and
  `borrow_bytes_ref(value) -> Result<&'call [u8], i32>` borrow the `Vec<u8>`
  passed as one of the call's arguments, from its raw argument value; the
  mutable form needs an argument declared `&mut [u8]` (`BytesMut`).

The lifetime `'call` is chosen by the generated wrapper for each call and is
higher-ranked: a function that tries to store a borrowed slice or the context
in a `static`, or that asks for `CallContext<'static>`, does not compile.

### Raw exports

A function can take the raw ABI arguments itself:

```rust,ignore
use aelys_native::{AelysValue, NativeContext, with_call_context};

#[aelys_export(params = [Int, Bytes], result = Int)]
pub unsafe extern "C" fn peek(
    context: *mut NativeContext,
    args: *const AelysValue,
    arg_count: usize,
    out: *mut AelysValue,
) -> i32 {
    let (index, frame) = unsafe { (*args, *args.add(1)) };
    let found = unsafe {
        with_call_context(context, |call| {
            let bytes = call.borrow_bytes_ref(frame)?;
            let index = call.read_int(index).ok_or(1)?;
            let byte = *bytes.get(usize::try_from(index).map_err(|_| 1)?).ok_or(1)?;
            call.int(i64::from(byte))
        })
    };
    match found {
        Ok(value) => {
            unsafe { *out = value };
            0
        }
        Err(code) => code,
    }
}
```

`params` lists the parameter types (`Int`, `Float`, `Bool`, `String`, `Unit`,
`Bytes` for `&[u8]`, `BytesMut` for `&mut [u8]`) and `result` the result type (`Int`, ..., `OptionInt`, ...,
`ResultUnitString`; `Unit` when omitted). The wrapper checks the argument
count before calling the function. `#[aelys_export(arity = N)]` alone declares
an untyped export: the VM can call it, but typed Elyra code cannot (E0379),
only a host through the embedding call API.

The return status of a raw function is `0` on success, with the result in
`*out`, or one of:

| status | constant | meaning |
|---|---|---|
| 1 | `AELYS_NATIVE_INVALID_ARGUMENT` | bad argument, or a byte borrow of a value that is not a byte argument of the call |
| 2 | `AELYS_NATIVE_INTEGER_OVERFLOW` | an int result the host cannot allocate (a host older than API version 5 boxes nothing) |
| 3 | `AELYS_NATIVE_RESULT_ERROR` | `Err`: `*out` holds the message string |
| 4 | `AELYS_NATIVE_OPTION_NONE` | `None` |
| 5 | `AELYS_NATIVE_BORROW_CONFLICT` | a `Vec<u8>` borrowed mutably while another borrow of it is live in the call |
| 6 | `AELYS_NATIVE_PANIC` | the function panicked |

## Borrow rules

- A borrowed slice is valid only until the native function returns. Never keep
  it, its pointer, or the `CallContext` (or the raw `NativeContext` pointer) in
  a static, a struct that outlives the call, or a thread that outlives the
  call. Scoped threads that finish before the function returns may work on a
  borrowed slice.
- Read-only borrows of one Vec coexist: a `&[u8]` parameter and
  `CallContext::borrow_bytes_ref` can view the same argument any number of
  times in one call (`sdl::present_indexed(&px, &px)` is accepted).
- A mutable borrow is exclusive. The compiler refuses `&mut v` next to another
  borrow of `v` in one call; the VM checks again, because a native called
  through a function value (`let f = pixels::copy_into` then `f(a, a)`) skips
  the compile-time check: a `&mut [u8]` parameter or `CallContext::borrow_bytes`
  on a Vec that already has any borrow in the call, or any borrow of a Vec
  already borrowed mutably, is refused with status 5, and the Elyra program
  stops with `native error: code 5 (byte buffer borrowed mutably while another
  borrow of it is live in the same native call)`.
- `CallContext::borrow_bytes` (mutable) only accepts an argument declared
  `&mut [u8]` (`BytesMut`), and both borrow functions only accept an argument
  declared `&[u8]` or `&mut [u8]`; anything else is status 1. A raw export
  declared with `arity` only may borrow any `Vec<u8>` it is handed.
- Any number of distinct Vecs can be borrowed in one call; up to 8 are
  tracked without allocating.
- A Vec cannot be resized while it is borrowed: no Elyra code runs during a
  native call.

## No re-entry into the VM

A native function cannot call Elyra code: there is no way to call a closure,
and the `register_function` / `register_constant` / `register_type` slots of the
VM API are empty. Design APIs that Elyra polls (`sdl::poll()` returns one event
per call) instead of callbacks. The VM API functions (`alloc_string`,
`read_string`, `borrow_bytes`, `borrow_bytes_shared`, `read_int`, `alloc_int`)
may only be called during a native call, with the context of that call. The
`size` field of the table tells a module which slots its host provides:
`read_int` and `alloc_int` were added at the end in API version 5.

## Threads

Natives run on the thread that runs the VM; the CLI runs it on the main thread,
which is what SDL video on macOS requires. `CallContext` is neither `Send` nor
`Sync`, and the VM API must not be called from another thread. A module that
keeps state between calls (a window, a texture) should keep it in a
`thread_local!`, as `modules/sdl` does.

## Errors

Return `Result<T, String>` for errors the Elyra program should handle and
`Option<T>` for absence. Any other non-zero status stops the program with
`native error: code N (...)`. A panic in a typed export (an out-of-bounds slice
index, an `unwrap`) is caught by the generated wrapper, which returns status 6;
the Elyra program stops with the runtime error `native function 'pixels::name'
panicked` and the host process keeps running. A raw export catches its panics
only when it is declared `unsafe extern "C-unwind" fn`; a panic inside a raw
`extern "C"` function cannot leave it and aborts the whole process.

## Building and loading

```sh
cargo build --release --manifest-path my-module/Cargo.toml
cp my-module/target/release/libmy_module.so game/pixels.so
```

`use pixels` in `game/main.aelys` finds `pixels.so` next to the script. A
manifest `main.aelys.toml` (or a project `aelys.toml`) can pin it:

```toml
[module.pixels]
kind = "native"
path = "lib/pixels.so"
required_version = "^0.1"
checksum = "<the checksum E0408 reports for the file>"
```

The loader checks the ABI version (`AELYS_ABI_VERSION`, currently 5), the
descriptor size and the hash of the export table, and rejects a signature with
an unknown type code, a sum type as a parameter or a byte slice as a result.
Type code 17 (`Bytes`) is `&[u8]` and 18 (`BytesMut`) is `&mut [u8]`; a module
built before code 18 existed declared `&mut [u8]` as code 17 and must be rebuilt,
or its mutable borrows are refused with status 1.
An embedding host registers a module statically with
`Runtime::register_native_module(descriptor)`.

## Performance: keep inner loops in Elyra

A loop iteration that makes one native call takes about 130 to 160 ns, against
about 60 ns for an empty iteration: the call itself is about 640 to 710
instructions (release build, measured with a zero-argument call and with a
call that borrows a 16-byte buffer). Cross the boundary once per frame or per batch, never per pixel or per
sample: hand a whole frame to `sdl::present_indexed`, keep the renderer, the
game logic and every per-element loop in Elyra.

## The SDL2 module

`modules/sdl` is a first-party module wrapping SDL2. It is not a member of the
workspace, so building Elyra never needs SDL2. It needs the SDL2 development
library (`libSDL2.so`, `pkg-config sdl2`).

```sh
cargo build --release --manifest-path modules/sdl/Cargo.toml
cp modules/sdl/target/release/libaelys_sdl.so examples/sdl/sdl.so
aelys run examples/sdl/gradient.aelys 120
```

A Doom E1M1 engine written in Elyra and drawn through this module lives in the
separate doom-elyra repository.

Without network access, copy the workspace `Cargo.lock` to
`modules/sdl/Cargo.lock` and add `--offline`.

| function | meaning |
|---|---|
| `open(title, width, height, scale) -> Result<(), string>` | window of `width*scale` x `height*scale` with a `width` x `height` ARGB streaming texture, nearest-neighbour scaling |
| `present_argb(&frame)` -> `Result<(), string>` | upload exactly `width*height*4` bytes, one little-endian `0xAARRGGBB` per pixel, and show them; any other size is an `Err` |
| `present_indexed(&pixels, &palette)` -> `Result<(), string>` | exactly `width*height` palette indices and a 768-byte RGB palette (any other size is an `Err`); the conversion to ARGB happens in the module, then the frame is shown |
| `poll() -> int` | next event: 0 none left, 1 quit, 2 key down, 3 key up, 4 anything else (key repeats included) |
| `event_key() -> int` | SDL scancode of the last key event `poll` returned |
| `key_down(scancode) -> bool` | current keyboard state of a scancode |
| `scancode(name) -> int` | scancode of an SDL key name (`"Escape"`, `"Up"`, `"Left Ctrl"`, `"Space"`), 0 when unknown |
| `ticks_ms() -> int` | milliseconds since SDL started |
| `now_ns() -> int` | high-resolution nanoseconds since the module's first clock read |
| `delay_ms(ms)` | sleep |
| `read_pixel(x, y) -> Result<int, string>` | `0xRRGGBB` of the last frame at screen pixel (x, y), read back from the renderer |
| `close()` | destroy the window and shut SDL down; `open` can be called again |

Run headless with `SDL_VIDEODRIVER=dummy`, or on a virtual X display
(`Xvfb :99` and `DISPLAY=:99`). The test `aelys/tests/t4_sdl_module.rs` builds
the module, runs `examples/sdl/gradient.aelys` with the dummy driver and
checks the pixels read back, then checks that frames of the wrong size are
refused. It builds a copy of the module under the cargo target directory (with
`--offline` and the workspace `Cargo.lock`), so it writes nothing into
`modules/sdl`. It fails when `pkg-config sdl2` fails, unless
`AELYS_SKIP_SDL_TESTS=1` is set.
