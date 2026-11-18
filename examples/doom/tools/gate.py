import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AELYS = os.environ.get("AELYS", "aelys-cli")

CONFIGS = {
    "headless-realtime": ("gate.aelys", ["--realtime"]),
    "headless-maxspeed": ("gate.aelys", []),
    "sdl-realtime": ("gate_sdl.aelys", ["--realtime"]),
    "sdl-maxspeed": ("gate_sdl.aelys", []),
}


def loadavg():
    with open("/proc/loadavg") as f:
        return " ".join(f.read().split()[:3])


def sample_rss(proc, out):
    page_kb = os.sysconf("SC_PAGE_SIZE") // 1024
    while proc.poll() is None:
        try:
            with open(f"/proc/{proc.pid}/statm") as f:
                out.append(int(f.read().split()[1]) * page_kb)
        except (OSError, ValueError, IndexError):
            pass
        time.sleep(1.0)


def parse(stdout):
    res = {}
    for line in stdout.splitlines():
        f = line.split()
        if len(f) < 2:
            continue
        if f[0] == "gate":
            key, rest = f[1], f[2:]
            if key in ("slowest_cause",):
                res[key] = " ".join(rest)
                continue
            if key in ("slowest", "spike"):
                res.setdefault(key, []).append(" ".join(rest))
                continue
            vals = {}
            for i in range(0, len(rest) - 1, 2):
                vals[rest[i]] = rest[i + 1]
            res[key] = vals
        elif f[0] == "gc-stats:" and f[1].startswith("collections="):
            res["gc_exit"] = dict(kv.split("=", 1) for kv in f[1:])
    return res


def run_one(aelys, config, jit, sdl_display, timeout, monsters, extra):
    script, flags = CONFIGS[config]
    flags = flags + (["--monsters"] if monsters else []) + extra
    cmd = [aelys, "run", "-ae.gc-stats=on"]
    if jit:
        cmd.append("-ae.jit=on")
    cmd += [os.path.join(HERE, script)] + flags
    env = dict(os.environ)
    if script == "gate_sdl.aelys":
        env["DISPLAY"] = sdl_display
    before = loadavg()
    proc = subprocess.Popen(cmd, cwd=HERE, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            stdin=subprocess.DEVNULL, text=True)
    rss = []
    sampler = threading.Thread(target=sample_rss, args=(proc, rss), daemon=True)
    sampler.start()
    try:
        stdout, _ = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        stdout, _ = proc.communicate()
    sampler.join(timeout=2)
    res = parse(stdout)
    res["exit"] = proc.returncode
    res["crashed"] = proc.returncode != 0 or "crashes" not in res
    res["loadavg_before"] = before
    res["loadavg_after"] = loadavg()
    res["rss_external_kb"] = {"first": rss[0] if rss else None, "last": rss[-1] if rss else None,
                              "max": max(rss) if rss else None, "samples": len(rss)}
    if res["crashed"]:
        res["tail"] = stdout[-2000:]
    return res


def num(res, key, field):
    try:
        return float(res[key][field])
    except (KeyError, ValueError, TypeError):
        return None


def summary(values):
    vals = [v for v in values if v is not None]
    if not vals:
        return "n/a"
    med = statistics.median(vals)
    return f"{med:g} ({min(vals):g}-{max(vals):g})"


def main():
    ap = argparse.ArgumentParser(description="Doom gate: 2100 tics of Freedoom E1M1 per run")
    ap.add_argument("--aelys", default=AELYS)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--configs", default="headless-realtime,headless-maxspeed,sdl-realtime")
    ap.add_argument("--jit", default="off,on", help="off, on or off,on")
    ap.add_argument("--sdl-module", help="built libaelys_sdl.so, copied next to the scripts as sdl.so for the sdl runs")
    ap.add_argument("--display", default=":99")
    ap.add_argument("--json", help="write every run's parsed figures here")
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--nomonsters", action="store_true", help="leave the monsters out (they are spawned by default)")
    ap.add_argument("--phases", action="store_true", help="record per-phase render time")
    args = ap.parse_args()

    configs = args.configs.split(",")
    jits = [j == "on" for j in args.jit.split(",")]
    sdl_target = os.path.join(HERE, "sdl.so")
    placed = False
    if any(c.startswith("sdl") for c in configs):
        if args.sdl_module:
            shutil.copyfile(args.sdl_module, sdl_target)
            placed = True
        elif not os.path.exists(sdl_target):
            sys.exit("sdl configs need --sdl-module or an sdl.so next to gate_sdl.aelys")
    results = {}
    try:
        for config in configs:
            for jit in jits:
                name = f"{config} jit={'on' if jit else 'off'}"
                runs = []
                for i in range(args.runs):
                    r = run_one(args.aelys, config, jit, args.display, args.timeout, not args.nomonsters,
                                ["--phases"] if args.phases else [])
                    runs.append(r)
                    fm = r.get("frame_ms", {})
                    print(f"{name} run {i + 1}: p50 {fm.get('p50')} p99 {fm.get('p99')} max {fm.get('max')} "
                          f"crashed {r['crashed']} load {r['loadavg_before']} -> {r['loadavg_after']}", flush=True)
                results[name] = runs
    finally:
        if placed:
            os.unlink(sdl_target)

    print()
    print("| config | p50 ms | p99 ms | max ms | p99 ms minus preemption | fps | gc max pause in run us | gc max pause vm us | rss first/last/max MB | alloc obj/s | crashes | loadavg |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for name, runs in results.items():
        ok = [r for r in runs if not r["crashed"]]
        rss = []
        for r in ok:
            rr = r.get("rss_kb", {})
            rss.append(f"{int(rr.get('first', 0)) // 1024}/{int(rr.get('last', 0)) // 1024}/{int(rr.get('max', 0)) // 1024}")
        print(f"| {name} | {summary([num(r, 'frame_ms', 'p50') for r in ok])} "
              f"| {summary([num(r, 'frame_ms', 'p99') for r in ok])} "
              f"| {summary([num(r, 'frame_ms', 'max') for r in ok])} "
              f"| {summary([num(r, 'cpu_frame_ms', 'p99') for r in ok])} "
              f"| {summary([num(r, 'run', 'fps') for r in ok])} "
              f"| {summary([num(r, 'gc_run', 'max_pause_us') for r in ok])} "
              f"| {summary([num(r, 'gc_vm', 'max_pause_us') for r in ok])} "
              f"| {', '.join(rss)} "
              f"| {summary([num(r, 'alloc_rate', 'objects_per_s') for r in ok])} "
              f"| {len(runs) - len(ok)} "
              f"| {'; '.join(r['loadavg_before'] for r in runs)} |")
    print()
    for name, runs in results.items():
        for i, r in enumerate(runs):
            if r["crashed"]:
                print(f"{name} run {i + 1} CRASHED exit {r['exit']}: {r.get('tail', '')[-400:]}")
                continue
            print(f"{name} run {i + 1} slowest: {(r.get('slowest') or ['?'])[0]}")
            print(f"    cause: {r.get('slowest_cause')}")
    if args.json:
        with open(args.json, "w") as f:
            json.dump(results, f, indent=1)
    if any(r["crashed"] for runs in results.values() for r in runs):
        sys.exit(1)


main()
