import collections
import pathlib
import re
import sys
import unicodedata

BAR_WIDTH = 40
TOP = 10

WORD = re.compile(r"[^\s'’]+(?:['’][^\s'’]+)*")


def lower_each(word):
    return "".join(c.lower() if len(c.lower()) == 1 else c for c in word)


def base_vowel(c):
    decomposed = unicodedata.normalize("NFD", c)
    return decomposed[0] if decomposed[0] in "aeiouy" and len(decomposed) > 1 else c


def main():
    text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
    letters_only = "".join(c if c.isalpha() or c in "'’" else " " for c in text)
    words = [lower_each(m.group(0)).replace("’", "'") for m in WORD.finditer(letters_only)]
    counts = collections.Counter(words)
    letters = collections.Counter(c for w in words for c in w if c.isalpha())

    out = []
    out.append(f"words: {len(words)} total, {len(counts)} distinct")
    if counts:
        best = min(counts, key=lambda w: (-len(w), w))
        out.append(f"longest: {best} ({len(best)} letters)")
    else:
        out.append("longest: none")

    top = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:TOP]
    width = max([0] + [len(w) for w, _ in top])
    out.append(f"top {TOP} words:")
    for i, (w, n) in enumerate(top, start=1):
        out.append(f"{i:>3}. {w:<{width}}  {n:>3}")

    accented = "".join(sorted(c for c in letters if base_vowel(c) != c)) or "none"
    out.append(
        f"letters: {sum(letters.values())} total, {len(letters)} distinct, "
        f"accented vowels: {accented}"
    )
    vowels = {v: 0 for v in "aeiouy"}
    for c, n in letters.items():
        b = base_vowel(c)
        if b in vowels:
            vowels[b] += n
    most = max([1] + list(vowels.values()))
    out.append("vowels (accents folded):")
    for v, n in vowels.items():
        out.append(f"  {v} {n:>4} {'#' * (n * BAR_WIDTH // most)}".rstrip())
    sys.stdout.write("\n".join(out) + "\n")


main()
