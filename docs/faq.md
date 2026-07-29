# FAQ

## Language Design

### Why 48-bit integers?

The VM uses NaN-boxing for value representation. A 64-bit float has certain bit patterns that represent NaN (Not a Number). Since there are many such patterns but we only need one NaN, I use the extras to encode other types.

After encoding type tags, there are 48 bits left for integer payloads. That's roughly ±140 trillion - enough for most purposes. If you need bigger numbers, use floats (with some precision loss) or wait until I add bigints.

## Performance

### Is Aelys fast?

The portable tier is a bytecode interpreter. On Linux x86_64, Aelys 0.22 additionally enables a tiered Cranelift JIT by default for supported typed integer and collection workloads.

Cold programs stay in the interpreter until a call or loop becomes hot. Hot loops can enter machine code through on-stack replacement, while unsupported operations safely continue in the interpreter.

Performance depends heavily on type information and workload shape. Run the repository's Criterion suite on the deployment machine before using a native module solely for speed.
### How does the GC work?

The heap is non-moving and generational. Minor collections trace young objects from roots and the remembered set; the old generation uses incremental mark-and-sweep slices with write barriers on mutations.

GC slices run at safepoints with a default 500 µs budget. Execution reports expose allocation, collection, and pause telemetry.

## Practical

### Is there a debugger?

Not yet but planned very soon. You can use `print` statements (sorry) or inspect bytecode with `aelys asm --stdout`.

### Can I embed Aelys in my Rust application?

Yes. The 0.22 API exposes a shared `Runtime`, immutable `CompiledModule`, and per-thread `Isolate`. Use `Runtime::compile`, `Runtime::new_isolate`, and `Isolate::execute`; configure fuel, deadlines, interruption, and reports through `RunOptions`.

### Are there tests?

Yes. Run `cargo xtask ci` for the reproducible local format, Clippy, workspace-test, and diff gate. `cargo xtask ci-full` adds release tests, locally available sanitizer/Miri/fuzz checks, and benchmark compilation.
