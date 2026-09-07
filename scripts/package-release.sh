#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
tag=${1:?Usage: package-release.sh TAG TARGET [BINARY]}
target=${2:?Missing target}
binary=${3:-target/$target/release/skillator}
case "$target" in
  aarch64-apple-darwin) baseline='macOS 14; deployment target 14.0' ;;
  x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) baseline='Ubuntu 24.04; glibc 2.39' ;;
  *) echo 'Unsupported release target' >&2; exit 1 ;;
esac
bash scripts/release-notes.sh "$tag" >/dev/null
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
artifact="skillator-${tag}-${target}"
cp "$binary" "$stage/skillator"
chmod 755 "$stage/skillator"
cp LICENCE.md "$stage/LICENSE"
# Keep the current tap working during the archive-layout transition. A hard
# link remains a valid executable when Homebrew moves it to bin/skillator.
ln "$stage/skillator" "$stage/$artifact"
{
  printf 'tag: %s\ncommit: %s\ntarget: %s\nfeatures: default\nbaseline: %s\n' \
    "$tag" "$(git rev-parse HEAD)" "$target" "$baseline"
  rustup run 1.97.1 rustc --version
  uname -srv
} > "$stage/BUILD.txt"
mkdir -p dist
# Packaging is repeatable from retained build inputs. Publication compares
# bytes and never assumes a rebuilt archive must equal a published archive.
COPYFILE_DISABLE=1 tar -czf "dist/$artifact.tar.gz" -C "$stage" skillator LICENSE BUILD.txt "$artifact"
(cd dist && shasum -a 256 "$artifact.tar.gz") > "dist/$artifact.tar.gz.sha256"
mkdir "$stage/extracted"
tar -xzf "dist/$artifact.tar.gz" -C "$stage/extracted"
cmp LICENCE.md "$stage/extracted/LICENSE"
bash scripts/smoke-test.sh "$stage/extracted/skillator" "${tag#v}"
# Exercise the exact legacy formula operation, including the hard-link move.
mkdir "$stage/bin"
mv "$stage/extracted/$artifact" "$stage/bin/skillator"
bash scripts/smoke-test.sh "$stage/bin/skillator" "${tag#v}"
