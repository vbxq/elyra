# Performance Benchmarks

Measurements on this page were taken on Linux x86_64; results vary by hardware.

## Interpreter and JIT

Run `cargo bench -p aelys --bench criterion_runtime` to compare the interpreter and the JIT locally (`--bench iai_runtime` counts instructions instead of time).

## Interpreter hot path

The interpreter runs the current frame in a dedicated hot loop. It covers register moves and loads, integer and float arithmetic (the wrapping `i8` to `u32` operations included), comparisons, branches and counted loops, `Vec` and array element loads and scalar stores, lengths, reads of initialised globals, scalar struct field loads and stores (`char` fields included), `Option` tests and payloads, reads and scalar writes of captured variables, and calls and returns between functions and closures. Every other instruction, and every edge case of a covered one (a non-integer operand, an overflow, a division by zero, an index out of bounds, an object store that needs a write barrier), leaves the hot loop before any effect and runs through the general loop. A runtime error reports the line of the instruction that raised it, the same in the hot loop, the general loop and the JIT. With `--max-instructions`, `--timeout-ms`, an interrupt handle or execution statistics enabled, only the general loop runs.

The compiler side writes a discarded assignment's value straight into its variable, uses immediate operands for `+ - >> & | ^` with a literal from 0 to 255, reads locals in place for field accesses and `if` conditions, and loads up to four wider literal operands of a loop once before the loop.

User-space machine instructions (`perf stat -e instructions:u`) per run of a set of micro-benchmarks kept outside the repository, release build, interpreter, before the hot loop existed (commit `198257d`) and with it:

| bench | before | after | ratio |
|---|---|---|---|
| int_loop (10 M iterations) | 13 234 M | 2 744 M | 4.8x |
| array_fill (20 M element operations) | 14 993 M | 4 334 M | 3.5x |
| nested_calls | 7 630 M | 2 639 M | 2.9x |
| struct_field | 18 436 M | 5 769 M | 3.2x |
| pixel_buffer (6.4 M pixels) | 10 704 M | 2 893 M | 3.7x |
| method_dispatch | 14 612 M | 5 343 M | 2.7x |
| fib(31) | 4 692 M | 1 825 M | 2.6x |
| fixed_mul | 6 984 M | 1 926 M | 3.6x |
| Doom renderer, one frame of E1M1 | 327 M | 84 M | 3.9x |
| closure call in a loop (1 M calls) | 1 555 M | 574 M | 2.7x |

With `-ae.jit=on`, functions the JIT refuses and loops it no longer enters stay on the interpreter's hot path.

## Startup

Running a file parses, type-checks and compiles it before the first instruction runs; `aelys compile` stores the result as `.avbc` bytecode so a later run skips that work. Measure a startup with:

```bash
time aelys-cli examples/hello.aelys
```

## Tiering and remaining fallbacks

- **Portable targets**: unsupported JIT targets execute the AVBC v3 interpreter.
- **Unsupported machine-code shapes**: closures, native/resource operations, and other operations the JIT does not compile fall back to the interpreter.
- **Cold code**: tiering deliberately avoids compilation before the hotness thresholds.
- **Allocation-heavy code**: the generational GC bounds incremental old-generation slices, but allocation and collection costs still need workload-specific measurement.

## Array and Vec storage

The VM uses specialized storage for each element type:
- `[int; N]`, `Vec<int>` and the other integer arrays and Vecs (all but `Vec<u8>`) → 8 bytes per element, no boxing
- `[float; N]`, `Vec<float>` → 8 bytes per element
- `[bool; N]`, `Vec<bool>` → 1 byte per element
- `Vec<u8>` → 1 byte per element (binary data)

The compiler emits type-specific opcodes too: `arr[i]` compiles to `ArrayLoadI` for an `int` element, `ArrayLoadF` for a `float` one, and so on, with no runtime type test.

### Vec growth

A Vec doubles its capacity when it runs out of space, so `push` is amortised O(1). Use `reserve(n)` to allocate once if you know the final size:

```rust
let mut v = vec![]
v.reserve(1000)  // one allocation instead of multiple
for i in 0..1000 {
    v.push(i)
}
```

### Tips

1. **Pre-allocate when you know the size**
   ```rust
   let mut v: Vec<int> = vec![]
   v.reserve(10000)
   ```

2. **Use Array for fixed-size data**
   ```rust
   let x = 1.0
   let y = 2.0
   let z = 3.0
   let coords = [x, y, z]  // simpler than Vec
   ```

3. **Batch operations**
   ```rust
   let mut arr = [1, 2, 3, 4]

   // One loop
   for i in 0..arr.len() {
       arr[i] = arr[i] * 2 + 1
   }

   // Instead of two
   for i in 0..arr.len() { arr[i] = arr[i] * 2 }
   for i in 0..arr.len() { arr[i] = arr[i] + 1 }
   ```

`examples/benchmark` holds small array, Vec, sort and matrix programs, most with a Python twin, to compare on your machine:

```bash
aelys-cli examples/benchmark/array_sum.aelys
python3 examples/benchmark/array_sum.py
```

## Improving Performance

If your Aelys code is too slow:

1. **Profile first** - make sure you're optimizing the right thing
2. **Turn the JIT on** - `-ae.jit=on` on Linux x86_64 for long-running numeric loops
3. **Pre-allocate arrays/vecs** - avoid reallocation overhead
4. **Precompile to bytecode** - skip parse/compile on each run

For truly performance-critical code, consider writing a native module in Rust and calling it from Aelys; see [Native modules](native-modules.md). Keep the loop itself in Aelys and hand the native a whole buffer: a native call costs far more than an interpreted instruction.
