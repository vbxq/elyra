# FAQ

## Language Design

### How big is an `int`?

`int` is a 64-bit signed integer whose arithmetic traps on overflow; `i64` and `u64` are the wrapping 64-bit types.

The VM uses NaN-boxing for value representation. A 64-bit float has certain bit patterns that represent NaN (Not a Number). Since there are many such patterns but we only need one NaN, I use the extras to encode other types. After the type tags there are 48 bits left, so an integer in -2^47..2^47 (about ±140 trillion) lives inside the value itself and costs nothing. A larger integer is stored as a small immutable heap object; it behaves exactly like a small one (comparisons, `Map` keys, sorting, printing are by value) and only costs an allocation when it is produced.

## Performance

### Is Aelys fast?

The portable tier is a bytecode interpreter. On Linux x86_64, Aelys 0.22 additionally enables a tiered Cranelift JIT by default for supported typed integer, floating-point, and collection workloads.

Cold programs stay in the interpreter until a call or loop becomes hot. Hot loops can enter machine code through on-stack replacement, while unsupported operations safely continue in the interpreter.

Performance depends heavily on type information and workload shape. Typed numeric calls to qualified native-module globals can cross the JIT through a checked runtime callback; general dynamic calls, object-valued calls, and unsupported operations remain explicit interpreter boundaries. Run the repository's Criterion suite on the deployment machine before using a native module solely for speed.
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
