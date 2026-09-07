#!/bin/bash
set -euo pipefail
tag=${1:?Usage: release-notes.sh TAG}
if [[ ! "$tag" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$ ]]; then
  echo 'Release tag must be v<semver>, optionally with a prerelease suffix' >&2
  exit 1
fi
version=$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)
test "$tag" = "v$version" || { echo 'Tag and Cargo.toml version differ' >&2; exit 1; }
awk -v version="$version" '
  /^## / { if (found) exit; if (index($0, "## [" version "] - ") == 1) { found=1; next } }
  found { print; if ($0 ~ /^### /) content=1 }
  END { if (!found || !content) exit 1 }
' CHANGELOG.md || { echo 'A dated, curated changelog section is required' >&2; exit 1; }
