import os
import re
import subprocess
import sys

AELYS = os.environ.get("AELYS", "/home/vbxq/.local/share/elyra-runs/bin/aelys-cli")

INSN = re.compile(r"^\s+(\d{4}): (\w+)\s*(.*?)\s*(?:;\s*(.*))?$")
LABEL = re.compile(r"^\s+(L\d+):\s*$")
NO_DEST = {"SetGlobalIdx", "VecStoreI", "VecStoreP", "ArrayStoreI", "ArrayStoreP", "JumpIfNotLong",
           "JumpIfLong", "JumpIfNot", "JumpIf", "Return", "VecPushI", "VecPushP", "VecReserve", "MatchFail",
           "StructStore", "StructStoreI", "StructStoreP"}
RANGE_READ = {"StructNew": (1, 2, 3), "VecLit": (0, 1, 2), "ArrayLit": (0, 1, 2)}


def regs(tokens):
    return [int(t[1:]) for t in tokens if re.fullmatch(r"r\d+", t)]


def parse(text):
    funcs = []
    cur = None
    for line in text.splitlines():
        m = re.match(r'^\s+\.name "(.*)"', line)
        if m:
            cur = {"name": m.group(1), "regs": 0, "insns": [], "labels": {}}
            funcs.append(cur)
            continue
        if cur is None:
            continue
        m = re.match(r"^\s+\.registers (\d+)", line)
        if m:
            cur["regs"] = int(m.group(1))
            continue
        m = LABEL.match(line)
        if m:
            cur["labels"][m.group(1)] = len(cur["insns"])
            continue
        m = INSN.match(line)
        if m:
            pc, op, args, comment = int(m.group(1)), m.group(2), m.group(3), m.group(4) or ""
            toks = [t.strip() for t in args.split(",") if t.strip()]
            cur["insns"].append({"pc": pc, "op": op, "toks": toks, "comment": comment})
    return funcs


def analyze(fn, frames):
    insns = fn["insns"]
    n = len(insns)
    pc_index = {ins["pc"]: i for i, ins in enumerate(insns)}
    for i, ins in enumerate(insns):
        op, toks = ins["op"], ins["toks"]
        r = regs(toks)
        reads, writes = set(), set()
        succ = [i + 1] if i + 1 < n else []
        if op == "Call":
            d, f, k = r[0], r[1], int(toks[2])
            reads = {f} | set(range(f + 1, f + 1 + k))
            writes = {d}
        elif op == "CallGlobal":
            d, k = r[0], int(toks[2])
            reads = set(range(d + 1, d + 1 + k))
            writes = {d}
        elif op in RANGE_READ:
            di, si, ci = RANGE_READ[op]
            nums = toks
            d = int(nums[di][1:])
            s = int(nums[si][1:])
            c = int(nums[ci])
            reads = set(range(s, s + c))
            writes = {d}
        elif op in ("ForLoopI", "ForLoopIInc", "VecForLoop", "ArrayForLoop", "StringForLoop"):
            base = r[0]
            if op.startswith("ForLoop"):
                reads = {base, base + 1, base + 2}
                writes = {base}
            else:
                reads = {base + 1, base + 2}
                writes = {base, base + 1}
            off = int(toks[1])
            tgt = pc_index.get(ins["pc"] + 1 + off)
            if tgt is not None:
                succ.append(tgt)
        elif op in NO_DEST:
            reads = set(r)
        else:
            if r:
                writes = {r[0]}
                reads = set(r[1:])
        for t in toks:
            if t in fn["labels"]:
                succ.append(fn["labels"][t])
        if op == "JumpLong":
            succ = [fn["labels"][t] for t in toks if t in fn["labels"]]
        if op == "Return":
            succ = []
        ins["reads"], ins["writes"], ins["succ"] = reads, writes, succ
    live_in = [set() for _ in range(n)]
    live_out = [set() for _ in range(n)]
    changed = True
    while changed:
        changed = False
        for i in range(n - 1, -1, -1):
            ins = insns[i]
            out = set()
            for s in ins["succ"]:
                out |= live_in[s]
            new_in = ins["reads"] | (out - ins["writes"])
            if out != live_out[i] or new_in != live_in[i]:
                live_out[i], live_in[i] = out, new_in
                changed = True
    problems = []
    for i, ins in enumerate(insns):
        if ins["op"] not in ("Call", "CallGlobal"):
            continue
        r = regs(ins["toks"])
        if ins["op"] == "Call":
            d, base = r[0], r[1] + 1
            callee = None
            for j in range(i - 1, -1, -1):
                pj = insns[j]
                if pj["op"] == "GetGlobalIdx" and regs(pj["toks"])[:1] == [r[1]]:
                    callee = pj["comment"].strip()
                    break
        else:
            d, base = r[0], r[0] + 1
            callee = ins["comment"].strip().rstrip("()").replace("__aelys_modglobal::", "")
        if not callee or callee not in frames:
            continue
        size = frames[callee]
        clobbered = sorted(x for x in live_out[i] if base <= x < base + size and x != d)
        if clobbered:
            problems.append((ins["pc"], callee, base, size, clobbered))
    return problems


def main():
    total = 0
    parsed = {}
    qualified = {}
    for path in sys.argv[1:]:
        text = subprocess.run([AELYS, "asm", path, "--stdout"], capture_output=True, text=True).stdout
        parsed[path] = parse(text)
        stem = os.path.splitext(os.path.basename(path))[0]
        for f in parsed[path]:
            qualified.setdefault(stem + "::" + f["name"], f["regs"])
    for path in sys.argv[1:]:
        funcs = parsed[path]
        frames = dict(qualified)
        for f in funcs:
            frames.setdefault(f["name"], f["regs"])
        seen = set()
        for f in funcs:
            key = (f["name"], len(f["insns"]))
            if key in seen:
                continue
            seen.add(key)
            for pc, callee, base, size, clob in analyze(f, frames):
                total += 1
                print(f"{path}: {f['name']} pc {pc:04d}: call {callee} frame r{base}..r{base + size - 1} clobbers live {['r%d' % x for x in clob]}")
    print(f"{total} clobbering call(s)")
    sys.exit(1 if total else 0)


main()
