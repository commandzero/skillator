#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
tag=${1:?Usage: publish-release.sh TAG [DIST]}
dist=${2:-dist}
: "${GITHUB_REPOSITORY:?Missing release repository}"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
bash scripts/release-notes.sh "$tag" > "$scratch/notes.md"
test "$(git rev-parse HEAD)" = "$(git rev-parse "refs/tags/$tag^{commit}")" || {
  echo 'HEAD must be the selected release tag' >&2
  exit 1
}
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo 'Release source has uncommitted tracked changes' >&2
  exit 1
fi
assets=()
for target in aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  archive="skillator-${tag}-${target}.tar.gz"
  test -f "$dist/$archive" && test -f "$dist/$archive.sha256"
  # Require one basename-only checksum, then validate the local bytes.
  test "$(wc -l < "$dist/$archive.sha256" | tr -d ' ')" = 1
  awk -v archive="$archive" 'NF != 2 || $2 != archive || length($1) != 64 || $1 ~ /[^0-9a-f]/ { exit 1 }' "$dist/$archive.sha256"
  (cd "$dist" && shasum -a 256 --check "$archive.sha256")
  assets+=("$dist/$archive" "$dist/$archive.sha256")
done
compare_downloads() {
  for asset in "${assets[@]}"; do
    name=$(basename "$asset")
    if ! test -f "$scratch/remote/$name" || ! cmp -s "$asset" "$scratch/remote/$name"; then
      echo "Existing release is missing or differs at $name; no assets were replaced. See docs/release.md." >&2
      return 1
    fi
  done
}
prerelease=false
[[ "$tag" != *-* ]] || prerelease=true
if gh release create "$tag" --repo "$GITHUB_REPOSITORY" --verify-tag --draft \
    --prerelease="$prerelease" --title "Skillator $tag" --notes-file "$scratch/notes.md"; then
  gh release upload "$tag" "${assets[@]}" --repo "$GITHUB_REPOSITORY"
  mkdir "$scratch/remote"
  gh release download "$tag" --repo "$GITHUB_REPOSITORY" --dir "$scratch/remote"
  compare_downloads
  gh release edit "$tag" --repo "$GITHUB_REPOSITORY" --draft=false
else
  # Fail closed on authentication/network errors and on incomplete releases.
  # An identical existing release is a successful no-op, including its metadata.
  mkdir "$scratch/remote"
  gh release download "$tag" --repo "$GITHUB_REPOSITORY" --dir "$scratch/remote"
  compare_downloads
  echo 'Existing release assets match. No publication or metadata change made.'
fi
