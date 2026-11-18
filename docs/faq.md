# FAQ

## Language Design

### Why 48-bit integers?

The VM uses NaN-boxing for value representation. A 64-bit float has certain bit patterns that represent NaN (Not a Number). Since there are many such patterns but we only need one NaN, I use the extras to encode other types.

After encoding type tags, there are 48 bits left for integer payloads. That's roughly ±140 trillion - enough for most purposes. If you need bigger numbers, use floats (with some precision loss) or wait until I add bigints.

## Performance

### Is Aelys fast?

The portable tier is a bytecode interpreter. On Linux x86_64 a Cranelift JIT is available; the `aelys` CLI turns it on with `-ae.jit=on` (embedders choose a `JitMode`).

A function is compiled after 1000 calls, and a loop is entered in machine code through on-stack replacement after 10000 back-edges in the same function, even when the function is called only once. Code is first compiled quickly with a single-pass register allocator; once it has run about three million loop iterations (a call counts as 16), it is compiled again with the optimizing allocator. A single activation that stays in one quick-tier loop for about a million iterations asks for the optimized code of that loop and moves to it at the loop header once it is ready. All the loop headers of a function are compiled together, into one piece of code with an entry per header.

With `-ae.jit=on` the CLI compiles on a background thread. When a function or loop crosses a threshold, the VM hands the compiler an owned copy of the function's bytecode and constants and keeps running the function in the interpreter (or in its quick code) meanwhile; the finished code is installed at the next call, loop back-edge or call from machine code, never in the middle of an instruction. Compiled code depends only on the bytecode: global values, function values held in globals, `Vec` storage and module globals are read and checked by the machine code when it runs, so a change made between the request and the install needs no invalidation. A panic inside the compiler marks that function as not compilable and the program goes on in the interpreter. `-ae.jit-sync=on` compiles on the calling thread instead, at the exact call or back-edge that crossed the threshold, so which code exists at a given point is deterministic; that is the default for embedders, who opt in with `JitConfig::with_async_compilation(true)`. The compiler thread is stopped when the last VM using it is dropped, after the compile it is running, and the memory of all compiled code is released then. Compiled code covers integer, float and bool arithmetic, bitwise operators (a left shift that leaves the 48-bit range traps as in the interpreter), integer division and remainder, `mul_shr` and `shl_div` on `int`, comparisons, `for` and `while` loops including `for` over a `Vec` or a fixed array, `Vec` and fixed-array element reads and writes for int, float, bool and object elements, `Vec` push, struct field reads, struct field writes of scalar fields (int, float, bool, unit), enum variant tests and payload reads, global reads and writes (including `+=` on a global), calls between Aelys functions and calls to native functions. Integer registers are kept unboxed inside a compiled region.

Machine code works on the interpreter's register window and reads globals from its function's own prepared view, which every global write keeps current. When it reaches an operation it does not compile (allocation, string operations, closures, sized-integer arithmetic, struct writes of non-scalar fields and others), or when an operation would trap (integer overflow, index out of bounds, division by zero, a read of a global whose initialiser has not run), it writes back the registers it changed, rebuilds the interpreter frames of every compiled callee on the stack and resumes the interpreter at that exact instruction. A native function that fails inside machine code reports its error after the same frames are rebuilt. Errors therefore carry the same message, source line and stack trace with or without the JIT. A loop or function stays compiled while each exit is paid for by at least four loop iterations or completed calls in machine code (after a budget of 64 exits); one that leaves more often than that is no longer entered, and its calls go to the older integer-only pipeline, which compiles only functions made of integer arithmetic, integer arrays and native calls, or else to the interpreter. A function that creates a closure capturing one of its locals is never compiled by the frame compiler.

### How does the GC work?

The heap is non-moving and generational. Every new object is young. A minor collection runs after a nursery's worth of allocation, traces young objects from the roots and the remembered set, frees the unreachable ones and promotes every survivor to the old generation. Its cost depends on the young objects and the remembered set, not on the size of the old heap.

The nursery adapts to the measured cost of each minor collection, in thread CPU time. It starts at a quarter of its limit (`-ae.gc-nursery=<size>`, 256 KiB by default). After each minor the VM computes its cost per nursery byte and keeps the highest recent value, which decays by 1/16 per minor; the next nursery is the size that this cost would collect in 0.5 ms, never above the limit nor below 16 KiB (or the limit, if it is smaller). The nursery therefore shrinks at once after an expensive minor and grows back by at most 1/15 per minor. A program whose new objects mostly survive gets frequent small minors.

Storing a pointer into an object runs a write barrier. An old object that receives a young value is remembered once, and a Vec or array is remembered per element, so the next minor collection reads only the written slots. The remembered set holds at most 4096 entries: past that, and whenever a young value is stored into a Vec or array without its index, the barrier promotes the stored value to the old generation instead (an object with children is then remembered once itself). A minor collection therefore reads at most 4096 entries plus the objects promoted since the previous one, whatever the number of stores. While a major collection is marking, the barrier also marks the stored value (insertion barrier), so marking finishes in bounded work however often a large Vec is rewritten.

A major collection starts when the heap reaches twice its size after the previous major (4 MiB at least), and at the latest when it reaches half of `-ae.max-heap`. It marks and then sweeps in slices interleaved with minor collections. The slices are paced so that the collection ends within one heap's worth of allocation, or within half of the room left below `max-heap` when that is smaller. One slice runs per 32 KiB allocated and stops after 1 ms; when a slice hits that deadline before its share of the work, the next slice comes after half as much allocation (2 KiB at least), and the interval doubles back once a slice keeps pace. Large Vecs and arrays are marked in chunks of 2048 elements. Only when an allocation would still exceed `max-heap` does the VM collect synchronously: first a minor collection, then, if that leaves less than a nursery of room, a complete major collection, whose pause grows with the heap.

#### Write barrier contract for runtime, native and JIT code

Code that writes into heap objects outside the interpreter's own store instructions must keep the collector informed:

- Every store or move of a value into an existing Vec or array slot calls `Heap::write_barrier_element(owner, final_index, value)` after the write. Operations that move elements (insert, remove, swap_remove, swap, reverse, sort, extend) call it for every element whose index changed, with its new index; `Heap::write_barrier_elements(owner, range)` does this for a range of slots after the move, reading the values back from the container. A move needs the barrier too: it remembers the new slot for the next minor collection, and during an incremental major it marks the value, which could otherwise cross the 2048-element chunk boundary of a Vec being marked and be swept.
- A store into any other object (struct field, closed upvalue, enum or sum payload, map entry, map key) calls `Heap::write_barrier_value(owner, value)`. A container whose entries move on their own, such as a hash map that rehashes, uses this whole-object form.
- `Heap::get_mut` performs no barrier.
- A new object kind that holds values must be traced in `collect_children` and, if it is indexed, in `element_values`.
- JIT code must not keep a heap reference only in a machine register across anything that can allocate: values live across an allocation point must be in VM registers or other roots.
- `Isolate` embedding code that holds a value across an allocation must pin it with a `HostRoot`.

`-ae.gc-verify=on` checks this contract: before each minor collection it walks the old generation and stops the VM with a `gc-verify` panic naming the owner and slot of any old-to-young pointer that no barrier recorded. It costs a walk of the whole heap per minor collection and is meant for tests, together with `-ae.gc-stress=on`.

`-ae.gc-stats=on` prints, when the VM exits, the number of minor and major collections, the maximum, p99 and total pause (wall-clock, plus the maximum and p99 in thread CPU time, which leaves out preemption by other processes), the allocation count and bytes, and a pause histogram. A program can read the same figures at run time with the `sys::gc_*` functions (see the standard library). `-ae.gc-stress=on` collects after almost every allocation and is meant for testing.

## Practical

### Is there a debugger?

Not yet but planned very soon. You can use `print` statements (sorry) or inspect bytecode with `aelys asm --stdout`.

### Can I embed Aelys in my Rust application?

Yes. The 0.22 API exposes a shared `Runtime`, immutable `CompiledModule`, and per-thread `Isolate`. Use `Runtime::compile`, `Runtime::new_isolate`, and `Isolate::execute`; configure fuel, deadlines, interruption, and reports through `RunOptions`. `CompiledModule::avbc()` can be persisted and restored with `Runtime::load_avbc()`.

Source-level `fn main(...)` is not an implicit entry point: defining it does not call it. Top-level code runs on `execute`; resolve a named function and call it for an explicit entry point. Module members use `::` in Aelys (`math::sqrt(...)`), while `.` selects values such as fields and methods.

### Are there tests?

Yes. Run `cargo xtask ci` for the reproducible local format, Clippy, workspace-test, and diff gate. `cargo xtask ci-full` adds release tests, locally available sanitizer/Miri/fuzz checks, and benchmark compilation.
