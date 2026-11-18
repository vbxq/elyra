import os
import subprocess
import sys

AELYS = os.environ.get("AELYS", "/home/vbxq/.local/share/elyra-runs/bin/aelys-cli")


def rss_kb(pid):
    with open(f"/proc/{pid}/status") as f:
        for line in f:
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    return -1


def main():
    script = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), sys.argv[1])
    proc = subprocess.Popen([AELYS, "run", script] + sys.argv[2:], stdout=subprocess.PIPE, text=True,
                            stdin=subprocess.DEVNULL, cwd=os.path.dirname(script))
    samples = []
    for line in proc.stdout:
        if line.startswith("mark "):
            kb = rss_kb(proc.pid)
            samples.append(kb)
            print(f"{line.strip()} rss_kb {kb}")
        elif not line.startswith(("trace", "sector")):
            sys.stdout.write(line)
    proc.wait()
    if samples:
        tail = samples[len(samples) // 4:]
        print(f"rss first {samples[0]} kb, min {min(samples)} kb, max {max(samples)} kb, "
              f"steady (after 25%) {min(tail)}..{max(tail)} kb")
    sys.exit(proc.returncode)


main()
