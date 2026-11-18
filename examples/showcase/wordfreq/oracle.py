import collections
import pathlib
import re
import sys
import unicodedata

HERE = pathlib.Path(__file__).resolve().parent
BAR_WIDTH = 40
TOP = 10

LETTER = r"[^\W\d_]"
WORD = re.compile(rf"{LETTER}+(?:['’]{LETTER}+)*")


def base_letter(c):
    decomposed = unicodedata.normalize("NFD", c)
    return decomposed[0] if decomposed[0] in "aeiouy" and len(decomposed) > 1 else c


def main():
    text = (HERE / "text.txt").read_text(encoding="utf-8")
    words = [m.group(0).lower().replace("’", "'") for m in WORD.finditer(text)]
    counts = collections.Counter(words)
    letters = collections.Counter(c for w in words for c in w if c.isalpha())

    out = []
    out.append(f"words: {len(words)} total, {len(counts)} distinct")
    best = min(counts, key=lambda w: (-len(w), w))
    out.append(f"longest: {best} ({len(best)} letters)")

    top = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:TOP]
    width = max(len(w) for w, _ in top)
    out.append(f"top {TOP} words:")
    for i, (w, n) in enumerate(top, start=1):
        out.append(f"{i:>3}. {w:<{width}}  {n:>3}")

    accented = sorted(c for c in letters if base_letter(c) != c)
    out.append(
        f"letters: {sum(letters.values())} total, {len(letters)} distinct, "
        f"accented: {''.join(accented)}"
    )
    vowels = {v: 0 for v in "aeiouy"}
    for c, n in letters.items():
        b = base_letter(c)
        if b in vowels:
            vowels[b] += n
    most = max([1] + list(vowels.values()))
    out.append("vowels (accents folded):")
    for v, n in vowels.items():
        out.append(f"  {v} {n:>4} {'#' * (n * BAR_WIDTH // most)}")
    sys.stdout.write("\n".join(out) + "\n")


main()
