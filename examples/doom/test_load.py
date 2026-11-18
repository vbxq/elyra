import difflib
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
AELYS = os.environ.get("AELYS", "/home/vbxq/.local/share/elyra-runs/bin/aelys-cli")
WAD = os.environ.get(
    "DOOM_WAD",
    "/home/vbxq/.local/share/elyra-runs/assets/fd/freedoom-0.13.0/freedoom1.wad",
)


def run(cmd):
    proc = subprocess.run(cmd, cwd=HERE, capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, timeout=600)
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout + proc.stderr)
        sys.exit(f"command failed: {' '.join(cmd)}")
    return proc.stdout


def main():
    expected = run([sys.executable, "-I", os.path.join(HERE, "oracle", "wad_oracle.py"), WAD])
    raw = run([AELYS, "run", os.path.join(HERE, "main.aelys"), WAD, "--time"])
    timing = [line for line in raw.splitlines() if line.startswith("time ")]
    # the cli echoes the script's final unit value as "()"
    actual = [line for line in raw.splitlines()
              if not line.startswith("time ") and line != "()"]
    want = expected.splitlines()
    if actual != want:
        sys.stdout.writelines(difflib.unified_diff(
            [l + "\n" for l in want], [l + "\n" for l in actual],
            "oracle", "elyra"))
        sys.exit(1)
    print("\n".join(actual))
    print("\n".join(timing))
    print(f"OK: {len(want)} summary lines match the oracle")


main()
