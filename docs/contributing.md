# Contributing

## Differential testing

`tools/difftest/run.py` generates random programs from several families (checked and
wrapping integers, sized integers, `&&`/`||`, char patterns, closures, nested places, `?`,
hot loops, tiered kernels, frame-JIT stress), runs each one under `-O0`, `-O2`, `-O3`,
`-ae.jit=on`, `-ae.jit-sync=on` and `-ae.gc-stress=on -ae.gc-verify=on`, and checks that
every mode prints the same thing. Most families also carry a Python oracle (the expected
stdout, or an expected integer trap) that every mode must match. It needs only Python 3.

```sh
cargo build --release
python3 tools/difftest/run.py --aelys target/release/aelys-cli --programs 50
```

`--programs N` is the number of programs per family. `--seed`, `--families a,b`,
`--modes O0,jit`, `--jobs` (default 2) and `--timeout-ms` (per run, default 30000) narrow
or widen a run. The exit code is non-zero when any program fails: modes that disagree,
a mismatch with the oracle, a VM panic, a timeout, or a program every mode refuses to
compile (a generator bug).

Each failing program is copied to `target/difftest-failures/` (or `--keep DIR`) with its
`.expected` or `.trap` file and a `.report.txt` holding every mode's command line, exit code,
stdout and stderr. To reproduce one, run the command from the report, for example
`target/release/aelys-cli run -ae.jit-sync=on checked_ints_0012.aelys`. Programs are named
`<family>_<index>` and are deterministic: the same `--seed` and `--first <index>
--programs 1 --families <family>` regenerate exactly that program.
