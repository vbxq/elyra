# Installation

## Requirements

- **Rust 1.85 or later** - the workspace uses the 2024 edition
- **Cargo** - comes with Rust

## Pre-built Binaries

The easiest way to get started is to download a pre-built binary from the [Releases page](https://github.com/vbxq/aelys_lang/releases).

Pick the binary for your platform and add it to your PATH.

## Building from Source

```bash
git clone https://github.com/vbxq/aelys_lang.git
cd aelys_lang
cargo build --release
```

The first build also compiles Cranelift, the JIT backend, so it takes a while. The binary lands in `target/release/aelys-cli`.

Add it to your PATH for convenience:

```bash
# In ~/.bashrc or ~/.zshrc
export PATH="$PATH:/path/to/aelys_lang/target/release"

# Or create a symlink
sudo ln -s /path/to/aelys_lang/target/release/aelys-cli /usr/local/bin/aelys
```

For Windows, you can add it manually to your PATH via System Properties, or copy the aelys-cli.exe file to your directory of choice.

## Verifying Installation

```bash
aelys-cli version
# Aelys v0.22.0
```

Run the hello world example:

```bash
aelys-cli examples/hello.aelys
# Hello, World!
# Welcome to Aelys v0.22.0!
# Platform: linux (x86_64)
```

## CLI Reference

### Basic Usage

```bash
aelys-cli <file.aelys>              # run a source file
aelys-cli run <file.aelys>          # same thing, explicit
aelys-cli run <file.avbc>           # run compiled bytecode
aelys-cli compile <file.aelys>      # compile to bytecode
aelys-cli asm <file.aelys>          # show disassembly
aelys-cli repl                      # interactive mode
aelys-cli version                   # show version
```

### Flags

**Optimization**

| Flag | Description |
|------|-------------|
| `-O0` | No optimization |
| `-O1` | Basic optimization |
| `-O2` | Standard optimization (default) |
| `-O3` | Aggressive optimization |

In practice, optimization levels don't make a huge difference for most code right now. The optimizer handles constant folding, dead code elimination, and some loop optimizations

**Output**

| Flag | Description |
|------|-------------|
| `-o <path>` | Output file (for compile/asm) |
| `--stdout` | Print asm to stdout instead of file |

**VM Options**

You can tune VM behavior with `-ae.<option>=<value>`:

```bash
aelys-cli -ae.max-heap=128M program.aelys
```

| Option | Description |
|--------|-------------|
| `max-heap=<size>` | Heap size limit (at least 1M) |
| `gc-nursery=<size>` | Largest allocation between two minor collections; the nursery adapts below it to keep minors near 0.5 ms (default 256K, at least 1K) |
| `gc-stats=on` | Print collection counts, pause maximum, p99, total and histogram, allocations and bytes when the program exits |
| `gc-stress=on` | Collect after almost every allocation (testing only) |
| `gc-verify=on` | Before each minor collection, check that every old-to-young pointer was recorded by a write barrier, and panic otherwise (testing only, walks the whole heap) |
| `jit=on` | Enable the Cranelift JIT (Linux x86_64), compiled on a background thread; off by default |
| `jit-sync=on` | Turn the JIT on and compile on the calling thread at the exact call or back-edge that crossed the threshold (deterministic, for tests and benchmarks); `-ae.jit=off` together with `-ae.jit-sync=on` is refused |

Sizes accept the suffixes `K`, `M` and `G`.

**Limits**

| Flag | Description |
|------|-------------|
| `--max-instructions <n>` | Stop with an error after `n` bytecode instructions |
| `--timeout-ms <n>` | Stop with an error after `n` milliseconds |

Under either limit the program runs in the interpreter's general loop only, without the JIT.

**Warnings**

| Flag | Description |
|------|-------------|
| `-Wall` | Enable all warnings |
| `-Werror` | Treat warnings as errors |
| `-W<category>` | Enable one category (`inline`, `unused`, `deprecated`, `shadow`, `type`) |
| `-Wno-<category>` | Disable one category |

See Compiler Warnings in the [language specification](language-spec.md#compiler-warnings).

### Examples

```bash
# Run with arguments passed to the program
aelys-cli program.aelys arg1 arg2

# Compile and save bytecode
aelys-cli compile main.aelys -o main.avbc

# Run compiled bytecode
aelys-cli run main.avbc

# View disassembly for debugging
aelys-cli asm main.aelys --stdout

# REPL with increased heap
aelys-cli repl -ae.max-heap=1G
```

## File Types

| Extension | Description |
|-----------|-------------|
| `.aelys` | Source code |
| `.avbc` | Compiled bytecode |
| `.aasm` | Assembly (human-readable bytecode) |

Bytecode files are portable across machines with the same Aelys version. They skip parsing and compilation.

A `.avbc` records the `use` lines its entry file declared, so running it imports the same modules the
source did, under the same names, whether or not the program qualifies them. The modules themselves are
not embedded: they are resolved beside the entry file, so a `.avbc` moved away from them will not run.

## Project Structure

Here's a sample project layout to explain module organization:

```
my_project/
  main.aelys           # entry point
  config.aelys         # configuration module
  lib/
    utils.aelys        # utility module
    http.aelys         # http helpers
  tests/
    test_utils.aelys   # tests
```

Modules are imported relative to the file that imports them. From `main.aelys`:

<!-- doctest: ignore (needs the project files shown above) -->
```rust
use config
use lib::utils
use lib::http
```

There's no package manager or dependency system for now, just files and directories.

## Editor Support

Soon :tm:

## Troubleshooting

### "module not found: 'foo'"

Module resolution is relative to the importing file; the note under the error lists every path that was tried. Check:
- Is the file path correct?
- Did you spell the module name right?
- For standard modules, use the exact path with `::` (`use std::fs`, not `use std.fs` or `use std::FS`)

### "error: expected X, found Y"

Parse error. Check:
- Missing braces?
- Typo in keyword?
- String not closed?

Error messages include line numbers. The compiler tries to be helpful but some messages could be clearer, soon (tm)

### "integer overflow"

An `int` operation left the 64-bit range (-9223372036854775808 to 9223372036854775807). Use `i64` or `u64` for wrapping arithmetic, or floats for approximate larger values.

### "invalid bytecode"

The bytecode file is corrupted or was compiled with a different Aelys version, try recompiling from source.

### The REPL is behaving weirdly

The REPL reads one line at a time, so a function, struct, enum, impl or trait has to fit on
one line. Every line sees what earlier lines defined: functions, structs, enums, impls, traits
and constants declared before, and globals (`let`) with the type they were given. Defining a
function or type again with the same name replaces the earlier definition for every later
line, including functions that call it. A line that fails to compile keeps none of its
definitions. A top-level `let` in the REPL is never an unused-value error, since a later line
may read it.

## Running Tests

If you want to verify the build:

```bash
cargo test
```

`cargo xtask ci` runs the formatting, Clippy and workspace test gate that CI runs.

## Documentation examples

The Aelys code blocks of `README.md` and `docs/*.md` are run by a test:

```bash
python3 tools/doctest.py --aelys target/release/aelys-cli
```

`cargo test -p aelys-cli --test doc_examples_tests` runs the same check against the freshly built binary. Every ```` ```rust ```` block must compile and run successfully, and a line comment `// prints: TEXT` declares an expected line of output, checked in order. A block that is not meant to run says so in its fence:

| Fence | Meaning |
|-------|---------|
| ```` ```rust,ignore ```` | not checked: a fragment, pseudo-code, or Rust code |
| ```` ```rust,no_run ```` | compiled but not run (network, endless loop, stdin) |
| ```` ```rust,compile_fail ```` | must be rejected by the compiler; `rust,compile_fail,E0323` also requires that code |
| ```` ```rust,should_panic ```` | must compile and then stop with a runtime error |

The same flags can be written in an HTML comment on the line before the fence, with a reason: `<!-- doctest: ignore (needs a WAD file) -->`. `<!-- doctest: file shapes.aelys -->` before a block saves it as that module for the later blocks of the same page instead of running it. `python3 tools/doctest.py --help` describes every option.
