#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/filenames-check.sh
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/docs/Nested" "$scratch/empty"
printf 'hidden/\n' > "$scratch/docs/.gitignore"
touch "$scratch/docs/Nested/lowercase.md"
check_lowercase_filenames "$scratch/docs" "$scratch/empty"
mkdir "$scratch/docs/hidden"
touch "$scratch/docs/hidden/Upper Case.md"
if check_lowercase_filenames "$scratch/docs" > "$scratch/output" 2>&1; then
  echo 'Ignored uppercase filename was accepted' >&2; exit 1
fi
rm "$scratch/docs/hidden/Upper Case.md"
touch "$scratch/docs/line"$'\n'"Upper.md"
if check_lowercase_filenames "$scratch/docs" > "$scratch/output" 2>&1; then
  echo 'Uppercase filename containing a newline was accepted' >&2; exit 1
fi
if check_lowercase_filenames "$scratch/missing" > "$scratch/output" 2>&1; then
  echo 'Failed directory scan was accepted' >&2; exit 1
fi
echo 'Filename checks passed.'
