# Native modules

A native module is a shared library (`.so`, `.dylib`, `.dll`) written against
the aelys-native ABI, usually in Rust with the `aelys-native` crate. Elyra
loads it with `needs`, type-checks every call against the signatures the
module declares, and calls it on the VM thread.

There is no generic C FFI: Elyra cannot `dlopen` an arbitrary library and call
a C symbol. To use a C library (SDL2, an audio API, a compressor), write a small
native module that wraps it. `modules/sdl` is the reference example.

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

```rust
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
name Elyra code uses (`needs pixels`, `pixels::checksum(buf)`), and it must also
be the file name the library is copied to (`pixels.so`).

### Types that cross the boundary

| Rust parameter | Elyra argument | notes |
|---|---|---|
| `i64` | `int` | 48-bit range |
| `f64` | `float` | |
| `bool` | `bool` | |
| `String` | `string` | copied into the native |
| `&[u8]` | byte buffer handle (`int`) | read-only view of the whole buffer |
| `&mut [u8]` | byte buffer handle (`int`) | writable view; writes are visible to Elyra after the call |
| `&mut CallContext<'_>` | nothing | not an Elyra argument; at most one per function |

| Rust result | Elyra type |
|---|---|
| `i64`, `f64`, `bool`, `String`, `()` | `int`, `float`, `bool`, `string`, unit |
| `Option<T>` (T one of the above) | `Option<T>` |
| `Result<T, String>` | `Result<T, string>` |

A byte buffer is the handle returned by `bytes::alloc`, `bytes::from_string`,
`fs::read_bytes` or `fs::read_file_bytes`. Structs, arrays, `Vec`, maps,
closures and raw pointers do not cross. Returning an `i64` outside the 48-bit
range is error code 2.

### The call context

`CallContext<'call>` is the native side of one call:

- `alloc_string(&str) -> Result<AelysValue, i32>` creates an Elyra string,
  which stays alive until the native function returns, however many values
  the call allocates after it (the raw `alloc_string_from_context` behaves the
  same way);
- `read_string(AelysValue) -> Option<String>` copies an Elyra string out;
- `borrow_bytes(handle) -> Result<&'call mut [u8], i32>` and
  `borrow_bytes_ref(handle) -> Result<&'call [u8], i32>` borrow a byte buffer
  from its raw handle value.

The lifetime `'call` is chosen by the generated wrapper for each call and is
higher-ranked: a function that tries to store a borrowed slice or the context
in a `static`, or that asks for `CallContext<'static>`, does not compile.

### Raw exports

A function can take the raw ABI arguments itself:

```rust
use aelys_native::{AelysValue, NativeContext, value_as_int, value_int, with_call_context};

#[aelys_export(params = [Int, Bytes], result = Int)]
pub unsafe extern "C" fn peek(
    context: *mut NativeContext,
    args: *const AelysValue,
    arg_count: usize,
    out: *mut AelysValue,
) -> i32 {
    let (index, handle) = unsafe { (*args, *args.add(1)) };
    let found = unsafe {
        with_call_context(context, |call| {
            let bytes = call.borrow_bytes_ref(handle)?;
            let byte = *bytes.get(value_as_int(index) as usize).ok_or(1)?;
            value_int(i64::from(byte)).ok_or(1)
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
`Bytes`) and `result` the result type (`Int`, ..., `OptionInt`, ...,
`ResultUnitString`; `Unit` when omitted). The wrapper checks the argument
count before calling the function. `#[aelys_export(arity = N)]` alone declares
an untyped export: the VM can call it, but typed Elyra code cannot (E0379),
only a host through the embedding call API.

The return status of a raw function is `0` on success, with the result in
`*out`, or one of:

| status | constant | meaning |
|---|---|---|
| 1 | `AELYS_NATIVE_INVALID_ARGUMENT` | bad argument or handle |
| 2 | `AELYS_NATIVE_INTEGER_OVERFLOW` | an int result outside 48 bits |
| 3 | `AELYS_NATIVE_RESULT_ERROR` | `Err`: `*out` holds the message string |
| 4 | `AELYS_NATIVE_OPTION_NONE` | `None` |
| 5 | `AELYS_NATIVE_BORROW_CONFLICT` | a byte buffer borrowed mutably while another borrow of it is live in the call |
| 6 | `AELYS_NATIVE_PANIC` | the function panicked |

## Borrow rules

- A borrowed slice is valid only until the native function returns. Never keep
  it, its pointer, or the `CallContext` (or the raw `NativeContext` pointer) in
  a static, a struct that outlives the call, or a thread that outlives the
  call. Scoped threads that finish before the function returns may work on a
  borrowed slice.
- Read-only borrows of one buffer coexist: a `&[u8]` parameter and
  `CallContext::borrow_bytes_ref` can view the same handle any number of times
  in one call (`sdl::present_indexed(px, px)` is accepted).
- A mutable borrow is exclusive: a `&mut [u8]` parameter or
  `CallContext::borrow_bytes` on a handle that already has any borrow in the
  call, or any borrow of a handle already borrowed mutably, is refused with
  status 5. Passing the same buffer as `&mut [u8]` and `&[u8]`
  (`pixels::copy_into(a, a)`) stops the Elyra program with `native error: code
  5 (byte buffer borrowed mutably while another borrow of it is live in the
  same native call)`.
- Any number of distinct buffers can be borrowed in one call; up to 8 are
  tracked without allocating.
- A buffer cannot be resized or freed while it is borrowed: no Elyra code runs
  during a native call and the ABI offers no way to free a buffer.
- A handle that is not a live byte buffer (a timer, a closed handle, any other
  int) is status 1.

## No re-entry into the VM

A native function cannot call Elyra code: there is no way to call a closure,
and the `register_function` / `register_constant` / `register_type` slots of the
VM API are empty. Design APIs that Elyra polls (`sdl::poll()` returns one event
per call) instead of callbacks. The VM API functions (`alloc_string`,
`read_string`, `borrow_bytes`, `borrow_bytes_shared`) may only be called during
a native call, with the context of that call.

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

`needs pixels` in `game/main.aelys` finds `pixels.so` next to the script. A
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
an unknown type code, a sum type as a parameter or a byte buffer as a result.
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

Without network access, copy the workspace `Cargo.lock` to
`modules/sdl/Cargo.lock` and add `--offline`.

| function | meaning |
|---|---|
| `open(title, width, height, scale) -> Result<(), string>` | window of `width*scale` x `height*scale` with a `width` x `height` ARGB streaming texture, nearest-neighbour scaling |
| `present_argb(frame)` -> `Result<(), string>` | upload exactly `width*height*4` bytes, one little-endian `0xAARRGGBB` per pixel, and show them; any other size is an `Err` |
| `present_indexed(pixels, palette)` -> `Result<(), string>` | exactly `width*height` palette indices and a 768-byte RGB palette (any other size is an `Err`); the conversion to ARGB happens in the module, then the frame is shown |
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
