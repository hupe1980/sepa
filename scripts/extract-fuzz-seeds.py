#!/usr/bin/env python3
"""Extract every ISO 20022 document literal in the tests into `fuzz/seeds/parse`.

Random bytes are never a well-formed ISO 20022 document, so the read-path fuzz
target explores nothing structural without seeds. These come from the same
fixtures the parser tests and the schema gate share, so a fixture that gains
coverage in one gains it in all three.

    just fuzz-seeds          regenerate after adding a fixture
    just fuzz-seeds-check    fail if the checked-in seeds are stale (CI)

The seeds are checked in because CI starts from a clean checkout and an
unseeded read-path target never reaches a well-formed document. They are also
*derived*, which is D38's situation exactly: a generated artefact that is
committed and never re-derived is a comment. So the derivation is verified in
CI rather than assumed.
"""

import hashlib
import pathlib
import re
import sys

SOURCES = [
    "tests/integration.rs",
    "tests/conformance.rs",
    "src/camt.rs",
    "src/camt029.rs",
    "src/camt052.rs",
    "src/camt053.rs",
    "src/camt054.rs",
    "src/pain002.rs",
]
LITERAL = re.compile(r'r#"(.*?)"#|r"([^"]*?)"', re.S)


def extract(root: pathlib.Path) -> dict[str, bytes]:
    """Every ISO 20022 document literal in the fixtures, keyed by content hash."""
    seeds: dict[str, bytes] = {}
    for name in SOURCES:
        text = (root / name).read_text()
        for match in LITERAL.finditer(text):
            literal = match.group(1) or match.group(2) or ""
            if "urn:iso:std:iso:20022" not in literal:
                continue
            # A format-string template is not a document until it is filled in.
            if "{" in literal and "}" in literal:
                continue
            data = literal.encode()
            seeds[hashlib.sha1(data).hexdigest()[:16]] = data
    return seeds


def main() -> None:
    check = "--check" in sys.argv
    root = pathlib.Path(__file__).resolve().parent.parent
    out = root / "fuzz" / "seeds" / "parse"
    out.mkdir(parents=True, exist_ok=True)

    want = extract(root)
    have = {f.name: f.read_bytes() for f in out.iterdir() if f.is_file()}

    if check:
        if want == have:
            print(f"fuzz/seeds/parse is up to date ({len(want)} seeds)")
            return
        missing = sorted(set(want) - set(have))
        extra = sorted(set(have) - set(want))
        print("fuzz/seeds/parse is STALE — run `just fuzz-seeds` and commit the result.")
        print(f"  {len(missing)} fixture(s) with no seed: {missing}")
        print(f"  {len(extra)} seed(s) with no fixture:  {extra}")
        print(
            "\nThe seeds are derived from the test fixtures. A fixture added "
            "without regenerating them is a document the read-path fuzzer never "
            "explores, which is how the sign-flip defect survived three releases."
        )
        sys.exit(1)

    for stale in out.iterdir():
        stale.unlink()
    for name, data in want.items():
        (out / name).write_bytes(data)
    print(f"{len(want)} unique seeds -> fuzz/seeds/parse")


if __name__ == "__main__":
    main()
