#!/bin/bash
set -euo pipefail
binary=${1:?Usage: smoke-test.sh BINARY VERSION}
version=${2:?Missing version}
binary="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
test "$("$binary" --version)" = "skillator $version"
mkdir -p "$scratch/home" "$scratch/library/example"
cat > "$scratch/library/example/SKILL.md" <<'SKILL'
---
name: example
description: Release smoke-test skill
---
Follow the example instructions.
SKILL
# HOME is only set for isolated child processes; no user configuration is read.
HOME="$scratch/home" "$binary" library add "$scratch/library"
listing=$(HOME="$scratch/home" "$binary" library list)
[[ "$listing" == *example* ]]
HOME="$scratch/home" "$binary" library remove "$scratch/library"
listing=$(HOME="$scratch/home" "$binary" library list)
[[ "$listing" != *example* ]]
