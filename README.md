<p align="center">
  <img src="docs/elyra_banner.png?v=2" alt="elyra virtual machine" width="1000">
</p>

# elyra 0.22.0

Register-based VM, a generational GC, isolated heaps, and a tiered Cranelift JIT on Linux x86_64.

## Relationship to Aelys

Elyra is the runtime Aelys originally ran on. When Aelys moved to an LLVM backend and a layered memory model, GC by default, with `nogc` regions that drop down to explicit ownership, the VM was split off into its own project rather than kept as dead weight inside a compiler that no longer used it.

Since the split, Elyra has become its own language. The design goal here is a runtime that is pleasant to build things on.

The executable is `elyra` and source files use `.elyra`. Internal Rust crate names and the native ABI still use Aelys.

## Quick start

```bash
cargo build --release
./target/release/elyra examples/hello.elyra
```

```rust
fn greet(name: string) -> string {
    "Hello, {name}!"
}

println(greet("Elyra"))  // prints: Hello, Elyra!
```

## Documentation

- [Installation](docs/installation.md): building, the command line, VM options
- [Getting started](docs/getting-started.md): a first tour of the language
- [Language specification](docs/language-spec.md): the complete reference, with every diagnostic code
- [Standard library](docs/standard-library.md): `std::io`, `std::math`, `std::string`, `std::time`, `std::fs`, `std::net`, `std::sys` and binary data in `Vec<u8>`
- [Native modules](docs/native-modules.md): extending Elyra with Rust shared libraries
- [Performance](docs/performance-benchmarks.md) and [FAQ](docs/faq.md)

# Notice

This is a personal project. I build it for fun, and I use it to make other things I find fun : [Raizen](https://github.com/vbxq/raizen_core), a bullet hell engine, a [CHIP-8 emulator](https://github.com/vbxq/chip8-aelys), that sort of thing.

It is not meant to be used seriously, and I would not recommend building anything you care about on top of it. There is no stability guarantee, no
release process, and a fair number of questionable decisions in here that exist because they were quick or because I felt like it that day.

There is also very little engineering novelty in Elyra. It is a fairly ordinary bytecode VM. If you came looking for new ideas in runtime design,
this is not the place, read the code if it's useful to you, but it is not trying to advance anything.

Aelys is where the experiments live → https://github.com/vbxq/aelys_lang