"""Flag corrupted text before it is delivered or committed.

Microverse is written in Spanish, so non-ASCII is normal here: em dashes,
arrows, accented letters, `<=`, `+-`, `2`. A naive "is it ASCII?" check is
useless. This one works the other way round: a character is **suspicious only
if it does not already appear somewhere in this repository**. The repo is the
corpus of what legitimate typography looks like; anything outside it is either
a corruption or a typo nobody has caught yet.

Two rules, in order:

1. **Hard denylist.** Characters from scripts this project never uses (Hangul,
   CJK, Cyrillic, Arabic, Hebrew, Devanagari, Thai, ...) and the fullwidth
   forms are always flagged, even if they somehow got committed once. The
   observed failure mode is a token landing in another language's
   distribution, so cross-script injection is exactly what we look for.
2. **Everything else non-ASCII** must be in the repo corpus. This catches
   mixed-script splices and mojibake without a hand-maintained allowlist.

It also flags C0 control characters and U+FFFD (the replacement character
mojibake turns into).

Usage:
    python3 tools/check_text.py src/lib.rs docs/plan_fase6.md
    python3 tools/check_text.py $(git diff --name-only)

Point it at what you are about to deliver or commit, not at the whole repo: the
`session-*.md` transcripts quote corrupted text on purpose (they are the record
of past bugs), so a repo-wide run reports those as hits. Source files, docs and
scripts should come back clean.

Exit code is 1 when something is flagged, 0 when clean, 2 on bad usage.

Known limit: this sees *characters*, not word splices. A corruption that stays
inside ASCII (`seinna` for "se ancla", `queJmply` for "simply") passes. Read
the diff; do not trust the exit code alone.
"""

from __future__ import annotations

import os
import sys
import unicodedata
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Directories that hold no source: build output, game data, VCS metadata.
SKIP_DIRS = {
    ".git",
    "target",
    "target-linux",
    "saves",
    "node_modules",
    "__pycache__",
}

# Extensions worth reading when building the corpus.
CORPUS_SUFFIXES = (
    ".rs",
    ".md",
    ".wgsl",
    ".toml",
    ".json",
    ".js",
    ".html",
    ".py",
    ".txt",
    ".yml",
    ".bat",
    ".sh",
)

# Files above this size are skipped when building the corpus.
MAX_CORPUS_BYTES = 4_000_000

# Codepoint ranges for scripts this project never writes in. Ranges, not
# characters: this catches anything a sampler might splice in. Greek is NOT
# here on purpose: `pi` and friends turn up in angle comments, and the corpus
# rule already rejects anything unusual in it.
DENIED_RANGES = (
    (0x0400, 0x04FF),  # Cyrillic
    (0x0530, 0x058F),  # Armenian
    (0x0590, 0x05FF),  # Hebrew
    (0x0600, 0x06FF),  # Arabic
    (0x0900, 0x097F),  # Devanagari
    (0x0E00, 0x0E7F),  # Thai
    (0x1100, 0x11FF),  # Hangul Jamo
    (0x3040, 0x30FF),  # Hiragana + Katakana
    (0x3130, 0x318F),  # Hangul compatibility Jamo
    (0x3400, 0x4DBF),  # CJK ext A
    (0x4E00, 0x9FFF),  # CJK unified
    (0xA960, 0xA97F),  # Hangul Jamo ext A
    (0xAC00, 0xD7A3),  # Hangul syllables
    (0xD7B0, 0xD7FF),  # Hangul Jamo ext B
    (0xF900, 0xFAFF),  # CJK compatibility
    (0xFB00, 0xFB4F),  # Alphabetic presentation forms (the "fi" ligature)
    (0xFF00, 0xFFEF),  # Fullwidth and halfwidth forms
    (0x20000, 0x2FA1F),  # CJK ext B+
)


def denied(char: str) -> str | None:
    """Why this character can never be legitimate here, or None."""
    code = ord(char)
    if char == "\t":
        return None
    if code < 0x20 or code == 0x7F:
        return f"control character U+{code:04X}"
    if code == 0xFFFD:
        return "replacement character U+FFFD (mojibake)"
    for low, high in DENIED_RANGES:
        if low <= code <= high:
            name = ""
            try:
                name = unicodedata.name(char)
            except ValueError:
                pass
            return f"script/width the repo never uses: {char!r} U+{code:04X} {name}"
    return None


def corpus_chars() -> set[str]:
    """Non-ASCII characters already present in the repository."""
    known: set[str] = set()
    for root, dirs, files in os.walk(REPO):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for name in files:
            if not name.endswith(CORPUS_SUFFIXES):
                continue
            path = Path(root) / name
            try:
                if path.stat().st_size > MAX_CORPUS_BYTES:
                    continue
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            known.update(c for c in text if ord(c) > 0x7E)
    return known


def check(paths: list[str], known: set[str]) -> int:
    found = 0
    for raw in paths:
        path = Path(raw)
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:
            print(f"{raw}: cannot read ({exc})")
            found += 1
            continue
        for number, line in enumerate(text.splitlines(), 1):
            for char in line:
                why = denied(char)
                if why is None and ord(char) > 0x7E and char not in known:
                    why = (
                        f"non-ASCII outside the repo corpus: {char!r} U+{ord(char):04X}"
                    )
                if why is not None:
                    found += 1
                    print(f"{raw}:{number}: {why}")
                    print(f"    {line.strip()[:160]}")
    print(f"--- {found} suspicious ---")
    return found


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__)
        return 2
    return 1 if check(argv[1:], corpus_chars()) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
