# FAQ

## Language Design

### Why 48-bit integers?

The VM uses NaN-boxing for value representation. A 64-bit float has certain bit patterns that represent NaN (Not a Number). Since there are many such patterns but we only need one NaN, I use the extras to encode other types.

After encoding type tags, there are 48 bits left for integer payloads. That's roughly ±140 trillion - enough for most purposes. If you need bigger numbers, use floats (with some precision loss) or wait until I add bigints.

## Performance

### Is Aelys fast?

The portable tier is a bytecode interpreter. On Linux x86_64 a Cranelift JIT is available; the `aelys` CLI turns it on with `-ae.jit=on` (embedders choose a `JitMode`).

A function is compiled after 1000 calls, and a loop is entered in machine code through on-stack replacement after 10000 back-edges in the same function, even when the function is called only once. Compiled code covers integer, float and bool arithmetic, bitwise operators, integer division and remainder, comparisons, `for` and `while` loops, `Vec` and fixed-array element reads and writes for int, float, bool and object elements, `Vec` push, struct field reads and writes, global reads and calls between Aelys functions.

Machine code works on the interpreter's register window. When it reaches an operation it does not compile (allocation, string operations, closures, native calls and others), or when an operation would trap (integer overflow, index out of bounds, division by zero), it writes the live registers back, rebuilds the interpreter frames of every compiled callee on the stack and resumes the interpreter at that exact instruction. Errors therefore carry the same message, source line and stack trace with or without the JIT. A function or loop that keeps leaving machine code is no longer entered.

### How does the GC work?

The heap is non-moving and generational. Minor collections trace young objects from roots and the remembered set; the old generation uses incremental mark-and-sweep slices with write barriers on mutations.

GC slices run at safepoints with a default 500 µs budget. Execution reports expose allocation, collection, and pause telemetry.

## Practical

### Is there a debugger?

Not yet but planned very soon. You can use `print` statements (sorry) or inspect bytecode with `aelys asm --stdout`.

### Can I embed Aelys in my Rust application?

Yes. The 0.22 API exposes a shared `Runtime`, immutable `CompiledModule`, and per-thread `Isolate`. Use `Runtime::compile`, `Runtime::new_isolate`, and `Isolate::execute`; configure fuel, deadlines, interruption, and reports through `RunOptions`. `CompiledModule::avbc()` can be persisted and restored with `Runtime::load_avbc()`.

Source-level `fn main(...)` is not an implicit entry point: defining it does not call it. Top-level code runs on `execute`; resolve a named function and call it for an explicit entry point. Module members use `::` in Aelys (`math::sqrt(...)`), while `.` selects values such as fields and methods.

### Are there tests?

Yes. Run `cargo xtask ci` for the reproducible local format, Clippy, workspace-test, and diff gate. `cargo xtask ci-full` adds release tests, locally available sanitizer/Miri/fuzz checks, and benchmark compilation.
