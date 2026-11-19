#!/usr/bin/env python3
import argparse
import concurrent.futures as cf
import importlib
import random
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(HERE))

from common import TRAP

FAMILIES = ["checked_ints", "wrapping64", "sized_ints", "hot_loops", "logic", "char_patterns", "closures",
            "places", "try_ops", "tiered_kernels", "frame_jit"]

MODES = {
    "O0": ["-O0"],
    "O2": ["-O2"],
    "O3": ["-O3"],
    "jit": ["-ae.jit=on"],
    "jit-sync": ["-ae.jit-sync=on"],
    "gc-stress": ["-ae.gc-stress=on", "-ae.gc-verify=on"],
}

PANIC_MARKERS = ("panicked", "type confusion", "type error in")
TRAP_MARKERS = ("integer overflow", "division by zero")
DEADLINE_MARKER = "execution deadline exceeded"


class Outcome:
    def __init__(self, rc, out, err, timed_out=False):
        self.rc, self.out, self.err, self.timed_out = rc, out, err, timed_out

    def kind(self):
        if self.timed_out or DEADLINE_MARKER in self.err:
            return "timeout"
        if any(m in self.err for m in PANIC_MARKERS):
            return "vm-panic"
        if self.rc != 0 and "error[E" in self.err:
            return "refused"
        return "ran"

    def error_key(self):
        # the message and its source location; the stack trace below is not compared
        keep = []
        for line in self.err.splitlines():
            s = line.strip()
            if s.startswith("Error:") or s.startswith("-->"):
                keep.append(s)
            if len(keep) == 2:
                break
        return " ".join(keep)

    def signature(self):
        return (self.rc == 0, self.out, self.error_key() if self.rc != 0 else "")

    def short(self):
        if self.timed_out:
            return "TIMEOUT (subprocess)"
        tail = self.out[-300:]
        err = self.error_key() or next((l.strip() for l in self.err.splitlines() if l.strip()), "")
        return f"rc={self.rc} stdout={tail!r} err={err[:200]!r}"


def run_one(aelys, path, mode, timeout_ms):
    cmd = [aelys, "run", *MODES[mode], "--timeout-ms", str(timeout_ms), path.name]
    try:
        r = subprocess.run(cmd, cwd=path.parent, capture_output=True, timeout=timeout_ms / 1000 * 3 + 30)
    except subprocess.TimeoutExpired:
        return Outcome(-1, "", "", timed_out=True)
    return Outcome(r.returncode, r.stdout.decode(errors="replace"), r.stderr.decode(errors="replace"))


def judge(expected, results):
    kinds = {m: o.kind() for m, o in results.items()}
    problems = []
    refused = [m for m, k in kinds.items() if k == "refused"]
    if refused and len(refused) == len(results):
        first = next(iter(results.values()))
        err = next((l.strip() for l in first.err.splitlines() if "error[E" in l), "")
        return [("generator-bug", f"refused by every mode: {err[:200]}")]
    for m, k in kinds.items():
        if k in ("timeout", "vm-panic"):
            problems.append((k, f"{m}: {results[m].short()}"))
    if refused:
        problems.append(("mode-difference", "refused only under " + ",".join(refused)))
    sigs = {}
    for m, o in results.items():
        if kinds[m] == "ran":
            sigs.setdefault(o.signature(), []).append(m)
    if len(sigs) > 1:
        groups = sorted(sigs.values(), key=len, reverse=True)
        detail = " | ".join(",".join(g) + ": " + results[g[0]].short() for g in groups)
        problems.append(("mode-difference", detail))
    if expected is not None:
        for m, o in results.items():
            if kinds[m] != "ran":
                continue
            if expected == TRAP:
                ok = o.rc != 0 and any(t in o.err for t in TRAP_MARKERS)
                want = "an integer trap"
            else:
                ok = o.rc == 0 and o.out == expected
                want = f"stdout={expected[-300:]!r}"
            if not ok:
                problems.append(("oracle-mismatch", f"{m}: expected {want}, got {o.short()}"))
    return problems


def write_program(dirpath, family, index, source, expected):
    path = dirpath / f"{family}_{index:04d}.aelys"
    path.write_text(source)
    if expected == TRAP:
        path.with_suffix(".trap").write_text("trap\n")
    elif expected is not None:
        path.with_suffix(".expected").write_text(expected)
    return path


def keep_failure(keep, aelys, path, expected, results, problems):
    keep.mkdir(parents=True, exist_ok=True)
    dest = keep / path.name
    shutil.copyfile(path, dest)
    for suffix in (".expected", ".trap"):
        if path.with_suffix(suffix).exists():
            shutil.copyfile(path.with_suffix(suffix), dest.with_suffix(suffix))
    report = []
    for cat, detail in problems:
        report.append(f"{cat}: {detail}")
    report.append("")
    for m, o in results.items():
        report.append(f"== {m}: {aelys} run {' '.join(MODES[m])} {dest.name}")
        report.append(f"rc={o.rc}{' timeout' if o.timed_out else ''}")
        report.append("-- stdout")
        report.append(o.out[-4000:])
        report.append("-- stderr")
        report.append(o.err[-4000:])
    dest.with_suffix(".report.txt").write_text("\n".join(report) + "\n")
    return dest


def main():
    ap = argparse.ArgumentParser(description="differential tests: generated programs run under every mode and a python oracle")
    ap.add_argument("--aelys", help="aelys binary to test")
    ap.add_argument("--programs", type=int, default=20, help="programs generated per family")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--families", default=",".join(FAMILIES))
    ap.add_argument("--modes", default=",".join(MODES))
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--timeout-ms", type=int, default=30000, help="per run VM deadline")
    ap.add_argument("--keep", help="directory receiving failing programs (default: target/difftest-failures)")
    ap.add_argument("--work", help="directory for the generated corpus, removed at the end (default: a temp dir)")
    ap.add_argument("--first", type=int, default=0, help="index of the first program of each family")
    ap.add_argument("--list", action="store_true", help="list the families and exit")
    args = ap.parse_args()
    if args.list:
        for f in FAMILIES:
            print(f"{f:15} {importlib.import_module(f).DESCRIPTION}")
        return 0
    if not args.aelys:
        ap.error("--aelys is required")

    families = [f for f in args.families.split(",") if f]
    modes = [m for m in args.modes.split(",") if m]
    for f in families:
        if f not in FAMILIES:
            ap.error(f"unknown family {f}; known: {', '.join(FAMILIES)}")
    for m in modes:
        if m not in MODES:
            ap.error(f"unknown mode {m}; known: {', '.join(MODES)}")
    aelys = str(Path(args.aelys).resolve())
    keep = Path(args.keep).resolve() if args.keep else HERE.parent.parent / "target" / "difftest-failures"
    if args.work:
        Path(args.work).mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="difftest-", dir=args.work))

    total_programs = total_runs = 0
    failures = []
    started = time.time()
    try:
        for family in families:
            mod = importlib.import_module(family)
            fdir = work / family
            fdir.mkdir()
            programs = []
            for index in range(args.first, args.first + args.programs):
                rng = random.Random(f"{args.seed}:{family}:{index}")
                source, expected = mod.generate(rng, index)
                programs.append((write_program(fdir, family, index, source, expected), expected))
            t0 = time.time()
            jobs = [(p, m) for p, _ in programs for m in modes]
            with cf.ThreadPoolExecutor(args.jobs) as ex:
                outs = list(ex.map(lambda j: run_one(aelys, j[0], j[1], args.timeout_ms), jobs))
            results = {}
            for (p, m), o in zip(jobs, outs):
                results.setdefault(p, {})[m] = o
            counts = {}
            nfail = 0
            for p, expected in programs:
                problems = judge(expected, results[p])
                oracle = "trap" if expected == TRAP else ("none" if expected is None else "stdout")
                counts[oracle] = counts.get(oracle, 0) + 1
                if problems:
                    nfail += 1
                    kept = keep_failure(keep, aelys, p, expected, results[p], problems)
                    failures.append((family, kept, problems))
            total_programs += len(programs)
            total_runs += len(jobs)
            oracle_s = " ".join(f"{k}={v}" for k, v in sorted(counts.items()))
            print(f"{family:15} {len(programs)} programs x {len(modes)} modes  oracle[{oracle_s}]  "
                  f"failing={nfail}  {time.time() - t0:.0f}s", flush=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)

    print(f"\n{total_programs} programs, {total_runs} runs, {len(failures)} failing programs, "
          f"{time.time() - started:.0f}s")
    for family, kept, problems in failures:
        print(f"\nFAIL {family} {kept}")
        for cat, detail in problems:
            print(f"  {cat}: {detail}")
    if failures:
        print(f"\nfailing programs and per-mode outputs kept in {keep}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
