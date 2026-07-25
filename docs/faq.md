# FAQ

## Language Design

### Why 48-bit integers?

The VM uses NaN-boxing for value representation. A 64-bit float has certain bit patterns that represent NaN (Not a Number). Since there are many such patterns but we only need one NaN, I use the extras to encode other types.

After encoding type tags, there are 48 bits left for integer payloads. That's roughly ±140 trillion - enough for most purposes. If you need bigger numbers, use floats (with some precision loss) or wait until I add bigints.

## Performance

### Is Aelys fast?

Depends what you mean by fast, it's a bytecode interpreter, so it won't match compiled languages or JIT-based runtimes.

For the fibonacci benchmark, it's roughly on par with Python. Node.js with V8 is about 10x faster. LuaJIT would be even faster.

Aelys is "fast enough" for scripting, tools, and applications where you're not CPU-bound. For tight numerical loops, consider writing a native module.

But I plan on writing a JIT (LLVM or Cranelift based) in the future to improve performance ! 
### How does the GC work?

Mark-and-sweep, stop-the-world, but I plan on using https://github.com/kyren/gc-arena in the future  

For most programs, pauses are imperceptible. Profile allocation-heavy workloads before optimizing them.

## Practical

### Is there a debugger?

Not yet but planned very soon. You can use `print` statements (sorry) or inspect bytecode with `aelys asm --stdout`.

### Can I embed Aelys in my Rust application?

Yes ! The `aelys` crate exposes the VM and compiler. The API isn't documented yet and might change, but it works. Look at the `aelys-cli` source for examples.

### Are there tests?

Yes, run `cargo test`. The test suite covers the compiler, VM, and standard library. There's 596 of them at the tiime of writing this (0.17.6-a)
-line input and module loading. It's functional but not polished, you can use the source files for now
