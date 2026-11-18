import math
import os
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
AELYS = os.environ.get("AELYS", "/home/vbxq/.local/share/elyra-runs/bin/aelys-cli")
WAD = os.environ.get("DOOM_WAD", "/home/vbxq/.local/share/elyra-runs/assets/fd/freedoom-0.13.0/freedoom1.wad")

MONSTERS = "--monsters" in sys.argv
EXPECTED_TICS = 3001 if MONSTERS else 2398
EXPECTED_FINAL = "final fixed 41674689 16355707 -8388608 48627712" if MONSTERS else "final fixed -4908963 102274657 -8388608 2281897984"
BLOCKED_BY_MONSTER = {"north door", "long hall", "outdoor", "west room", "l room"}
ROOMS = {
    "start room": {140},
    "big room": {90, 147, 149, 150},
    "south door": {100},
    "south hall": {57, 104},
    "lift": {103},
    "water room": {120, 125, 171},
    "channel": {17, 142},
    "north door": {10},
    "long hall": {13, 56},
    "outdoor": {0, 28},
    "west room": {8},
    "l room": {36, 135},
}


def load_map():
    data = open(WAD, "rb").read()
    _, count, dirofs = struct.unpack_from("<4sii", data, 0)
    lumps = []
    for i in range(count):
        pos, size, raw = struct.unpack_from("<ii8s", data, dirofs + 16 * i)
        lumps.append((raw.split(b"\0")[0].decode("latin-1").upper(), pos, size))
    m = next(i for i, l in enumerate(lumps) if l[0] == "E1M1")

    def lump(name):
        _, p, s = next(l for l in lumps[m + 1:m + 11] if l[0] == name)
        return data[p:p + s]

    verts = list(struct.iter_unpack("<hh", lump("VERTEXES")))
    lines = list(struct.iter_unpack("<HHHHHHH", lump("LINEDEFS")))
    walls = []
    for v1, v2, flags, _, _, _, back in lines:
        if back == 0xFFFF or flags & 1:
            walls.append((verts[v1], verts[v2]))
    return walls


def seg_dist(p, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    t = ((p[0] - ax) * dx + (p[1] - ay) * dy) / float(dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    return math.hypot(p[0] - ax - t * dx, p[1] - ay - t * dy)


def crosses(p, q, a, b):
    def orient(u, v, w):
        return (v[0] - u[0]) * (w[1] - u[1]) - (v[1] - u[1]) * (w[0] - u[0])

    d1, d2 = orient(a, b, p), orient(a, b, q)
    d3, d4 = orient(p, q, a), orient(p, q, b)
    return d1 * d2 < 0 and d3 * d4 < 0


def run(args):
    proc = subprocess.run([AELYS, "run", os.path.join(HERE, "game.aelys")] + args, cwd=HERE,
                          capture_output=True, text=True, stdin=subprocess.DEVNULL, timeout=1800)
    if proc.returncode != 0:
        sys.stdout.write(proc.stdout[-4000:] + proc.stderr[-4000:])
        sys.exit(f"game.aelys {' '.join(args)} failed with exit {proc.returncode}")
    return proc.stdout


def check_trace(out):
    walls = load_map()
    pts = []
    for line in out.splitlines():
        f = line.split()
        if f and f[0] == "trace":
            pts.append((int(f[1]), int(f[2]), int(f[3]), int(f[4]), int(f[7])))
    problems = []
    if len(pts) != EXPECTED_TICS:
        problems.append(f"{len(pts)} traced tics, expected {EXPECTED_TICS}")
    start = (-416, 256)
    prev = start
    for tic, x, y, z, _ in pts:
        if math.hypot(x - prev[0], y - prev[1]) > 31:
            problems.append(f"tic {tic}: jump of {math.hypot(x - prev[0], y - prev[1]):.1f} units")
        for a, b in walls:
            if min(a[0], b[0]) - 20 > max(x, prev[0]) or max(a[0], b[0]) + 20 < min(x, prev[0]):
                continue
            if min(a[1], b[1]) - 20 > max(y, prev[1]) or max(a[1], b[1]) + 20 < min(y, prev[1]):
                continue
            if seg_dist((x, y), a, b) < 14.5:
                problems.append(f"tic {tic}: center {x},{y} within {seg_dist((x, y), a, b):.1f} of wall {a}-{b}")
            if crosses(prev, (x, y), a, b):
                problems.append(f"tic {tic}: path {prev}->{x},{y} crosses wall {a}-{b}")
        prev = (x, y)
    return pts, problems


def main():
    mode = ["--monsters"] if MONSTERS else []
    out = run(["--logic", "--trace", "--sectors"] + mode)
    pts, problems = check_trace(out)
    final = [l for l in out.splitlines() if l.startswith("final fixed")]
    if final != [EXPECTED_FINAL]:
        problems.append(f"final state {final}, expected {EXPECTED_FINAL!r}")
    tics = [l for l in out.splitlines() if l.startswith("tics ")]
    if not tics or not tics[0].startswith(f"tics {EXPECTED_TICS} "):
        problems.append(f"tic count line {tics}")
    visited = {int(l.split()[1]) for l in out.splitlines() if l.startswith("sector ")}
    rooms = {r: v for r, v in ROOMS.items() if not (MONSTERS and r in BLOCKED_BY_MONSTER)}
    for room, sectors in rooms.items():
        if not visited & sectors:
            problems.append(f"room {room} (sectors {sorted(sectors)}) never visited")
    for line in out.splitlines():
        if line.startswith(("final", "tics", "tic logic", "demo use")):
            print(line)
    print(f"visited {len(visited)} sectors, rooms: {', '.join(rooms)}")
    if "--snaps" in sys.argv:
        snap_out = run(["--timedemo", "--snaps"] + mode)
        ppms = [l.split()[1] for l in snap_out.splitlines() if l.startswith("snap ")]
        subprocess.run([sys.executable, "-I", os.path.join(HERE, "tools", "ppm2png.py")] + ppms, check=True)
        for line in snap_out.splitlines():
            if line.startswith(("snap", "frame ms", "slowest", "tic logic", "final fixed")):
                print(line)
        final_td = [l for l in snap_out.splitlines() if l.startswith("final fixed")]
        if final_td != [EXPECTED_FINAL]:
            problems.append(f"timedemo final state {final_td} differs from the logic run")
    if problems:
        print("\n".join(problems[:40]))
        sys.exit(f"FAIL: {len(problems)} problem(s)")
    print(f"OK: demo completes in {EXPECTED_TICS} tics ({EXPECTED_TICS / 35:.1f} s), no wall contact, all rooms visited")


main()
