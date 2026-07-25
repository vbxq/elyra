<p align="center">
  <img src="docs/aelys_banner.png?v=2" alt="aelys virtual machine" width="1000">
</p>

# aelys 0.21.0

Register-based VM with dual memory management: GC by default, `@no_gc` for performance-critical code.

You choose between comfort and performance on a per-function basis.

# Two versions of Aelys

Before moving to LLVM, Aelys was implemented using a custom virtual machine.  
That original implementation now lives in this repository and is maintained separately.

The VM and LLVM versions share the same name and history, but are now distinct projects.  
This repository preserves the original VM-based version and allows it to evolve independently.
