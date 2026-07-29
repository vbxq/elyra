<p align="center">
  <img src="docs/aelys_banner.png?v=2" alt="aelys virtual machine" width="1000">
</p>

# aelys 0.22.0

Register-based VM with AVBC v2, a generational garbage collector, isolated heaps, and a tiered Cranelift JIT on Linux x86_64.

The public embedding API is organized around a shared `Runtime`, immutable `CompiledModule`s, and single-threaded `Isolate`s. `Runtime::new()` enables tiered JIT compilation on Linux x86_64 and falls back to the interpreter on unsupported targets; `JitMode::Off` and `JitMode::Baseline` remain available explicitly.

Aelys runs code selected by its user and is not a security sandbox. Native modules and the `sys`, `fs`, `net`, and `exec` facilities must be treated with the same trust as the host application.

# Two versions of Aelys

Before moving to LLVM, Aelys was implemented using a custom virtual machine.  
That original implementation now lives in this repository and is maintained separately.

The VM and LLVM versions share the same name and history, but are now distinct projects.  
This repository preserves the original VM-based version and allows it to evolve independently.
