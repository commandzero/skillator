#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
repo=$PWD
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/scripts" "$scratch/bin" "$scratch/dist"
cp scripts/release-publish.sh scripts/release-notes-generate.sh "$scratch/scripts/"
cat > "$scratch/Cargo.toml" <<'TOML'
[package]
version = "0.2.0"
TOML
cat > "$scratch/CHANGELOG.md" <<'LOG'
## [0.2.0] - 2026-09-06
### Fixed
- A tested fixture.
LOG
git -C "$scratch" init --quiet
git -C "$scratch" config user.name 'Release fixture'
git -C "$scratch" config user.email release@example.invalid
git -C "$scratch" add Cargo.toml CHANGELOG.md
git -C "$scratch" -c core.hooksPath=/dev/null commit --quiet -m 'test: release fixture'
git -C "$scratch" tag v0.2.0
for target in aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
  name="skillator-v0.2.0-$target.tar.gz"
  printf 'fixture\n' > "$scratch/dist/$name"
  (cd "$scratch/dist" && shasum -a 256 "$name") > "$scratch/dist/$name.sha256"
done
cat > "$scratch/bin/gh" <<'MOCK'
#!/bin/bash
set -eu
printf '%s\n' "$*" >> "$CALLS"
case "$2" in
  create) test "$CASE" = new ;;
  upload) ;;
  download)
    dest=''
    while [ "$#" -gt 0 ]; do
      if [ "$1" = --dir ]; then dest=$2; break; fi
      shift
    done
    test "$CASE" != network || exit 1
    cp "$FIXTURE"/dist/* "$dest/"
    if [ "$CASE" = different ]; then printf 'changed' >> "$dest/skillator-v0.2.0-aarch64-apple-darwin.tar.gz"; fi
    if [ "$CASE" = partial ]; then rm "$dest/skillator-v0.2.0-aarch64-apple-darwin.tar.gz"; fi
    ;;
  edit) ;;
  *) exit 1 ;;
esac
MOCK
chmod +x "$scratch/bin/gh"
export PATH="$scratch/bin:$PATH" FIXTURE="$scratch" GITHUB_REPOSITORY=fixture/skillator CALLS="$scratch/calls"
for mode in new same different partial network; do
  : > "$CALLS"
  if CASE="$mode" bash "$scratch/scripts/release-publish.sh" v0.2.0 > "$scratch/output" 2>&1; then
    [[ "$mode" = new || "$mode" = same ]] || { cat "$scratch/output"; exit 1; }
  else
    [[ "$mode" = different || "$mode" = partial || "$mode" = network ]] || { cat "$scratch/output"; exit 1; }
  fi
  if [ "$mode" = new ]; then
    rg -q 'release edit' "$CALLS"
  else
    if rg -q 'release (upload|edit)' "$CALLS"; then echo 'Existing release was mutated' >&2; exit 1; fi
  fi
  if rg -q -- '--clobber' "$CALLS"; then exit 1; fi
done
# A dirty source or a checkout beyond the tag must fail before GitHub writes.
: > "$CALLS"
printf '\nUncommitted edit\n' >> "$scratch/CHANGELOG.md"
if CASE=new bash "$scratch/scripts/release-publish.sh" v0.2.0 > "$scratch/output" 2>&1; then exit 1; fi
test ! -s "$CALLS"
git -C "$scratch" restore CHANGELOG.md
git -C "$scratch" -c core.hooksPath=/dev/null commit --allow-empty --quiet -m 'test: later commit'
if CASE=new bash "$scratch/scripts/release-publish.sh" v0.2.0 > "$scratch/output" 2>&1; then exit 1; fi
test ! -s "$CALLS"
git -C "$scratch" checkout --detach --quiet v0.2.0
# An incomplete matrix must fail before contacting GitHub.
rm "$scratch/dist/skillator-v0.2.0-aarch64-apple-darwin.tar.gz"
: > "$CALLS"
if CASE=new bash "$scratch/scripts/release-publish.sh" v0.2.0 > "$scratch/output" 2>&1; then exit 1; fi
test ! -s "$CALLS"
# Smoke validation must reject both a no-op removal and a failed list command.
cat > "$scratch/bin/skillator" <<'MOCK'
#!/bin/bash
set -eu
case "$*" in
  --version) echo 'skillator 0.2.0' ;;
  'library add '*) touch "$HOME/registered" ;;
  'library remove '*) [[ "$SMOKE_CASE" = noop ]] || rm "$HOME/registered" ;;
  'library list')
    [[ "$SMOKE_CASE" != list-failure ]] || exit 1
    if [[ -f "$HOME/registered" ]]; then echo example; fi
    ;;
  *) exit 1 ;;
esac
MOCK
chmod +x "$scratch/bin/skillator"
for mode in valid noop list-failure; do
  if SMOKE_CASE="$mode" bash "$repo/scripts/release-smoke-test.sh" "$scratch/bin/skillator" 0.2.0; then
    [[ "$mode" = valid ]] || { echo "Smoke test accepted $mode" >&2; exit 1; }
  else
    [[ "$mode" != valid ]] || { echo 'Valid smoke fixture failed' >&2; exit 1; }
  fi
done
# Invalid and mismatched tags must fail before notes are used.
cd "$scratch"
for tag in v0.2.1 bad v01.2.0; do
  if bash "$repo/scripts/release-notes-generate.sh" "$tag" >/dev/null 2>&1; then exit 1; fi
done
echo 'Release safeguards passed: new, identical, differing, partial, network failure, wrong/dirty source, missing matrix, invalid tags.'
