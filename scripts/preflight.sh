#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tools-versions.sh
source scripts/filenames-check.sh
export PATH="$PWD/.tools/bin:$PWD/.tools/node_modules/.bin:$PATH"
export OPENSPEC_TELEMETRY=0
# Resolve via rustup even when a system Cargo precedes its shims on PATH.
cargo() { rustup run "${RUST_TOOLCHAIN:-1.97.1}" cargo "$@"; }
policy_checks() {
  shellcheck scripts/*.sh
  # The pinned actionlint release prints its bare version on the first line.
  actionlint -version | head -n 1 | rg -x "$ACTIONLINT_VERSION"
  actionlint
  okf --version | rg -F "okf $OKF_VERSION "
  check_lowercase_filenames docs
  bash scripts/filenames-test.sh
  okf validate docs/
  test "$(openspec --version)" = "$OPENSPEC_VERSION"
  # Open changes belong to individual PRs; unrelated drafts do not block CI.
  openspec validate --specs --strict --no-interactive
  bash scripts/release-test.sh
}
case "${1:-all}" in
  all)
    cargo fmt --check
    cargo clippy --all-targets --locked -- -D warnings
    cargo test --all-targets --locked
    cargo test --doc --locked
    policy_checks
    ;;
  test)
    cargo test --all-targets --locked
    cargo test --doc --locked
    ;;
  policy)
    policy_checks
    cargo test --example repo-check --locked
    ;;
  pr)
    : "${PR_BASE:?Set PR_BASE to the target commit}"
    : "${PR_HEAD:?Set PR_HEAD to the PR head commit}"
    : "${PR_TITLE:?Set PR_TITLE to the proposed squash title}"
    : "${PR_BODY_FILE:?Set PR_BODY_FILE to a file containing the PR body}"
    test "$(openspec --version)" = "$OPENSPEC_VERSION"
    cargo run --quiet --locked --example repo-check -- pr "$PR_BASE" "$PR_HEAD" "$PR_TITLE" "$PR_BODY_FILE"
    ;;
  *) echo 'Usage: scripts/preflight.sh [all|test|policy|pr]' >&2; exit 2 ;;
esac
