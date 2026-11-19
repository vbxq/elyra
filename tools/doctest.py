#!/usr/bin/env python3
# checks the aelys code blocks of README.md and docs/*.md against an aelys binary

USAGE = """\
usage: python3 tools/doctest.py --aelys target/release/aelys-cli [--jobs N] [--filter TEXT] [-v]

Every fenced block whose info string starts with `rust` is checked. The info
string takes comma-separated flags:

  ```rust                    the block must compile and run with exit status 0
  ```rust,ignore             not checked: a fragment, pseudo-code, or real Rust
  ```rust,no_run             must compile, is not run (network, endless loop, stdin)
  ```rust,compile_fail       must be rejected by the compiler
  ```rust,compile_fail,E0323 rejected, and the code must appear in the diagnostic
  ```rust,should_panic       must compile, and the run must exit with a non-zero status

The same flags can be given by an html comment on the line just before the
fence, with an optional reason in parentheses:

  <!-- doctest: ignore (reads a file the reader provides) -->

Expected output: a line comment `// prints: TEXT`, after a statement or on a
line of its own, declares that TEXT is a line of standard output. The declared
lines must appear in the output in the order they are written; other output
lines may come in between.

Module files: a block preceded by `<!-- doctest: file NAME -->` is not run; it
is written as NAME next to every later block of the same markdown file, so an
example that imports a module can run.

Blocks that are real Rust (native module code) are skipped when they contain
Rust-only syntax; the summary counts them so they can be marked explicitly.

A block that defines `fn main` without calling it gets a `main()` call
appended, since the aelys cli runs top-level code and never calls `main`.
Each block runs in a fresh temporary directory, with a timeout.
"""

import argparse
import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

FENCE = re.compile(r"^(?P<indent>[ \t]*)(?P<ticks>`{3,})(?P<info>[^`]*)$")
DIRECTIVE = re.compile(r"^\s*<!--\s*doctest:\s*(?P<body>.*?)\s*-->\s*$")
PRINTS = re.compile(r"//\s*prints:\s?(?P<text>.*?)\s*$")
CODE = re.compile(r"^E\d{4}$")
KNOWN_FLAGS = {"ignore", "no_run", "compile_fail", "should_panic"}

RUST_ONLY = [
    re.compile(r"#\[\s*aelys_(export|module)"),
    re.compile(r"\buse\s+aelys_native\b"),
    re.compile(r"\bextern\s+\"C"),
    re.compile(r"\bunsafe\s+(extern\s+\S+\s+)?fn\b"),
    re.compile(r"\bfn\s+\w+(<[^>]*>)?\([^)]*&mut\s+\w+<'"),
    re.compile(r"\bimpl\s*<'\w+>"),
    re.compile(r"\bpub\s+fn\b[^{]*->\s*Result<[^>]*\bString\b"),
    re.compile(r"\b(i64|u8|usize)::"),
    re.compile(r"^\s*mod\s+\w+\s*\{", re.M),
]


class Block:
    def __init__(self, path, line, flags, codes, reason, source, file_name=None):
        self.path = path
        self.line = line
        self.flags = flags
        self.codes = codes
        self.reason = reason
        self.source = source
        self.file_name = file_name
        self.fixtures = {}

    @property
    def label(self):
        return f"{self.path}:{self.line}"


def parse_flags(text):
    flags, codes, unknown = set(), [], []
    for part in (p.strip() for p in text.split(",")):
        if not part:
            continue
        if part in KNOWN_FLAGS:
            flags.add(part)
        elif CODE.match(part):
            codes.append(part)
        else:
            unknown.append(part)
    return flags, codes, unknown


def extract(path, rel, errors):
    lines = path.read_text(encoding="utf-8").splitlines()
    blocks = []
    fixtures = {}
    i = 0
    while i < len(lines):
        m = FENCE.match(lines[i])
        if not m:
            i += 1
            continue
        ticks, indent = m.group("ticks"), m.group("indent")
        info = m.group("info").strip()
        start = i
        body = []
        i += 1
        while i < len(lines):
            close = FENCE.match(lines[i])
            if close and close.group("ticks") == ticks and not close.group("info").strip():
                break
            line = lines[i]
            body.append(line[len(indent):] if line.startswith(indent) else line.lstrip())
            i += 1
        i += 1
        lang, _, rest = info.partition(",")
        if lang.strip() != "rust":
            continue
        flags, codes, unknown = parse_flags(rest)
        reason = None
        file_name = None
        if start > 0:
            d = DIRECTIVE.match(lines[start - 1])
            if d:
                text = d.group("body")
                r = re.search(r"\((.*)\)\s*$", text)
                if r:
                    reason = r.group(1)
                    text = text[: r.start()]
                fm = re.match(r"^file\s+(\S+)\s*$", text.strip())
                if fm:
                    file_name = fm.group(1)
                else:
                    more, more_codes, more_unknown = parse_flags(text)
                    flags |= more
                    codes += more_codes
                    unknown += more_unknown
        label = f"{rel}:{start + 1}"
        if unknown:
            errors.append(f"{label} FAIL unknown doctest flag(s): {', '.join(unknown)}")
        if codes and "compile_fail" not in flags:
            errors.append(f"{label} FAIL error code given without compile_fail")
        block = Block(rel, start + 1, flags, codes, reason, "\n".join(body) + "\n", file_name)
        block.fixtures = dict(fixtures)
        if file_name:
            fixtures[file_name] = block.source
        blocks.append(block)
    return blocks


def looks_like_rust(source):
    return any(p.search(source) for p in RUST_ONLY)


def prepare(source):
    defines_main = re.search(r"^\s*(pub\s+)?fn\s+main\s*\(", source, re.M)
    if defines_main and not re.search(r"^\s*main\s*\(", source, re.M):
        params = re.search(r"^\s*(?:pub\s+)?fn\s+main\s*\(([^)]*)\)", source, re.M).group(1).strip()
        source += "main(Vec::new())\n" if params else "main()\n"
    return source


def expected_lines(source):
    out = []
    for line in source.splitlines():
        m = PRINTS.search(line)
        if m:
            out.append(m.group("text"))
    return out


def run(cmd, cwd, timeout):
    try:
        p = subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout,
            stdin=subprocess.DEVNULL, errors="replace",
        )
        return p.returncode, p.stdout, p.stderr, False
    except subprocess.TimeoutExpired as e:
        out = e.stdout.decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
        err = e.stderr.decode(errors="replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        return None, out, err, True


def first_error(stderr):
    for line in stderr.splitlines():
        if line.strip():
            return line.strip()[:200]
    return "(no diagnostic)"


def check(block, aelys, work, timeout, idx):
    d = Path(tempfile.mkdtemp(prefix=f"b{idx}-", dir=work))
    try:
        for name, text in block.fixtures.items():
            target = d / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")
        src = d / "main.aelys"
        src.write_text(prepare(block.source), encoding="utf-8")
        if "compile_fail" in block.flags:
            code, out, err, to = run([aelys, "compile", "main.aelys", "-o", "main.avbc"], d, timeout)
            if to:
                return False, "compiler timed out"
            if code == 0:
                return False, "expected a compile error, but it compiled"
            missing = [c for c in block.codes if c not in err]
            if missing:
                return False, f"compile error lacks {', '.join(missing)}: {first_error(err)}"
            return True, "compile_fail"
        if "no_run" in block.flags or "should_panic" in block.flags:
            code, out, err, to = run([aelys, "compile", "main.aelys", "-o", "main.avbc"], d, timeout)
            if to:
                return False, "compiler timed out"
            if code != 0:
                return False, f"compile error: {first_error(err)}"
            if "no_run" in block.flags:
                return True, "compiled"
        code, out, err, to = run([aelys, "run", "main.aelys"], d, timeout)
        if to:
            return False, f"timed out after {timeout}s"
        if "should_panic" in block.flags:
            if code == 0:
                return False, "expected a runtime error, but it exited with status 0"
            return True, f"panicked: {first_error(err)}"
        if code != 0:
            return False, f"exit {code}: {first_error(err)}"
        want = expected_lines(block.source)
        if want:
            got = [l.rstrip() for l in out.splitlines()]
            pos = 0
            for w in want:
                while pos < len(got) and got[pos] != w:
                    pos += 1
                if pos == len(got):
                    shown = " | ".join(got[:8]) + (" ..." if len(got) > 8 else "")
                    return False, f"expected output line {w!r} not found in order; stdout: {shown}"
                pos += 1
            return True, f"ok, {len(want)} output line(s) checked"
        return True, "ok"
    finally:
        shutil.rmtree(d, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(
        description="check the Aelys code blocks of the documentation",
        epilog=USAGE,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("--aelys", required=True, help="path to the aelys cli binary")
    ap.add_argument("--root", default=None, help="repository root (default: parent of tools/)")
    ap.add_argument("--jobs", type=int, default=2)
    ap.add_argument("--timeout", type=float, default=60.0, help="seconds per block")
    ap.add_argument("--filter", default=None, help="only blocks whose file:line contains this text")
    ap.add_argument("--work-dir", default=None, help="where the per-block directories are created")
    ap.add_argument("-v", "--verbose", action="store_true", help="also list skipped blocks")
    args = ap.parse_args()

    root = Path(args.root) if args.root else Path(__file__).resolve().parent.parent
    aelys = str(Path(args.aelys).resolve())
    if not os.access(aelys, os.X_OK):
        print(f"doctest: {aelys} is not an executable", file=sys.stderr)
        return 2

    files = [root / "README.md"] + sorted((root / "docs").glob("*.md"))
    errors = []
    blocks = []
    for f in files:
        if f.exists():
            blocks += extract(f, f.relative_to(root).as_posix(), errors)

    counts = {"found": len(blocks), "run": 0, "no_run": 0, "compile_fail": 0, "should_panic": 0,
              "ignored": 0, "rust": 0, "files": 0, "output": 0, "failed": 0}
    todo = []
    for b in blocks:
        if args.filter and args.filter not in b.label:
            continue
        if b.file_name:
            counts["files"] += 1
            if args.verbose:
                print(f"{b.label} file {b.file_name}")
        elif "ignore" in b.flags:
            counts["ignored"] += 1
            if args.verbose:
                print(f"{b.label} ignored" + (f" ({b.reason})" if b.reason else ""))
        elif looks_like_rust(b.source):
            counts["rust"] += 1
            print(f"{b.label} skipped (looks like Rust, mark it rust,ignore or change the fence)")
        else:
            todo.append(b)

    work = Path(args.work_dir) if args.work_dir else Path(tempfile.gettempdir())
    work.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="aelys-doctest-", dir=work))
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
            futures = [pool.submit(check, b, aelys, work, args.timeout, i) for i, b in enumerate(todo)]
            for b, fut in zip(todo, futures):
                ok, msg = fut.result()
                if "compile_fail" in b.flags:
                    counts["compile_fail"] += 1
                elif "should_panic" in b.flags:
                    counts["should_panic"] += 1
                elif "no_run" in b.flags:
                    counts["no_run"] += 1
                else:
                    counts["run"] += 1
                    if ok and expected_lines(b.source):
                        counts["output"] += 1
                if not ok:
                    counts["failed"] += 1
                print(f"{b.label} {'ok' if ok else 'FAIL'} {msg}" if not ok or args.verbose
                      else f"{b.label} ok")
    finally:
        shutil.rmtree(work, ignore_errors=True)

    for e in errors:
        print(e)
    failed = counts["failed"] + len(errors)
    print(
        f"doctest: {counts['found']} blocks, {counts['run']} run "
        f"({counts['output']} with checked output), {counts['no_run']} compiled only, "
        f"{counts['compile_fail']} compile_fail, {counts['should_panic']} should_panic, "
        f"{counts['ignored']} ignored, {counts['rust']} skipped as Rust, "
        f"{counts['files']} module files; {failed} failed"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
