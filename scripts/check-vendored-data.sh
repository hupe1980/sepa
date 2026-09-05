#!/usr/bin/env bash
#
# Verify the vendored ISO 20022 / GBIC schemas against the digests recorded in
# tests/xsd/README.md.
#
# Two failure modes, both of which matter:
#
#   1. A file's content no longer matches its recorded digest — it was edited,
#      re-fetched from a different source, or line endings were normalised.
#   2. A file has no recorded digest at all. That is the one that lets a
#      defective mirror in unnoticed: a schema that flattens `xs:choice` into
#      `xs:sequence` makes *correct* output fail validation, and the natural
#      reaction is to change the writer.
#
# The README is the single record; this script only reads it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
xsd_dir="$root/tests/xsd"
readme="$xsd_dir/README.md"

if command -v sha256sum >/dev/null 2>&1; then
  digest() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  digest() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  echo "error: neither sha256sum nor shasum is available" >&2
  exit 2
fi

# Pull `<file> <sha256>` pairs out of the README, whichever of the two shapes
# they are written in: a table row, or a prose bullet spanning two lines.
pairs="$(
  tr '\n' ' ' <"$readme" |
    grep -oE '`((pain|camt)\.[0-9A-Za-z._]+\.xsd)`[^`]{0,40}`[0-9a-f]{64}`' |
    sed -E 's/`([^`]+)`[^`]*`([0-9a-f]{64})`/\1 \2/'
)"

status=0
recorded=""

while read -r file expected; do
  [ -n "$file" ] || continue
  recorded="$recorded $file"
  path="$xsd_dir/$file"
  if [ ! -f "$path" ]; then
    echo "MISSING  $file — a digest is recorded but the file is not vendored" >&2
    status=1
    continue
  fi
  actual="$(digest "$path")"
  if [ "$actual" != "$expected" ]; then
    echo "MISMATCH $file" >&2
    echo "         recorded $expected" >&2
    echo "         actual   $actual" >&2
    status=1
  else
    echo "ok       $file"
  fi
done <<<"$pairs"

# Completeness: every vendored schema must be accounted for. Same argument as
# the Max*Text table and the character-set walk — a check whose coverage is a
# hand-written list silently stops covering things.
for path in "$xsd_dir"/*.xsd; do
  file="$(basename "$path")"
  case " $recorded " in
    *" $file "*) ;;
    *)
      echo "UNRECORDED $file — vendored with no SHA-256 in tests/xsd/README.md" >&2
      status=1
      ;;
  esac
done

if [ "$status" -ne 0 ]; then
  echo >&2
  echo "Vendored reference data does not match its record. See tests/xsd/README.md." >&2
fi
exit "$status"
