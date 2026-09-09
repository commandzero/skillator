---
type: Playbook
title: Releasing Skillator
description: Release compatibility, packaging, publication, and recovery procedures.
status: draft
generated: { by: codex/gpt-6, at: 2026-09-07T06:07:29Z }
---

# Releasing Skillator

Release only a reviewed tag whose manifest, lockfile, changelog, and compatibility notes agree.
The workflow builds a complete matrix, tests extracted packages, creates a draft, verifies uploaded bytes, then publishes.
Run the commands in this guide from the repository root.

## Compatibility and platforms

For 0.x releases, put incompatible public changes in the next minor version and compatible fixes in a patch release.
Public contracts include commands, exit statuses, report fields, configuration, and installed-file behavior.

Keep Rust 1.97.0 as the minimum compiler and 1.97.1 as the development/release compiler.
Increase the minimum in a minor release with a changelog entry and dependency checks. Declare OS/ABI minimum changes before release.

The next release uses this matrix. The workflow runs natively on each target; these are validation requirements, not a claim that a future CI run has passed.

| Target | Build and test host | Supported binary baseline |
| --- | --- | --- |
| aarch64-apple-darwin | macOS 14 arm64 | macOS 14, deployment target 14.0 |
| x86_64-unknown-linux-gnu | Ubuntu 24.04 x86_64 | Ubuntu 24.04, glibc 2.39 |
| aarch64-unknown-linux-gnu | Ubuntu 24.04 arm64 | Ubuntu 24.04, glibc 2.39 |

Newer compatible systems may run these binaries. Other Linux distributions need their own installation checks.
WSL uses the Linux build on its Linux filesystem; mounted Windows filesystems can lack required rename or link capabilities.

Native Windows, Intel macOS downloads, and musl downloads are outside this release matrix.
Source installation on other Unix hosts is not a substitute for testing their filesystem capabilities.

## Archive migration

Keep existing v0.1.0 assets, tags, checksums, and Homebrew URLs unchanged.
The planned first release with the new layout is v0.1.1; if release review selects another version, update this plan before tagging it.

Outer names remain unchanged:

```text
skillator-v<version>-<rust-target-triple>.tar.gz
skillator-v<version>-<rust-target-triple>.tar.gz.sha256
```

At the archive root, include `skillator`, `LICENSE`, and `BUILD.txt`.
The repository license lives in `LICENCE.md`; packaging keeps the archive name `LICENSE`.
BUILD.txt records the tag, source commit, compiler, default feature set, target, OS baseline, and build host.

Retain the old archive-stem executable as a hard link to `skillator` during the transition.
The existing CommandZero Homebrew formula selects that name and moves it into its bin directory. Package tests exercise that exact move and then run a functional skill-library check.

CommandZero/skillator owns the producer. CommandZero/homebrew-tools owns the consumer.
The compatibility hard link lets its current installer work with both generations of archive.

In the next formula update, prefer `bin.install "skillator"` when present and keep the legacy fallback for old releases.
Remove the compatibility link only in a later coordinated release after the tap and any other installers stop using it. That removal needs its own reviewed version and migration note.

## Release procedure

1. Prepare a PR with the selected Cargo.toml/Cargo.lock version, a dated CHANGELOG.md section, comparison links, compatibility notes, and any tap migration changes. Update this plan's first migration version if needed.
2. Pass the required PR checks and review any associated OpenSpec completion receipts. Merge the reviewed proposal, then create its matching version tag. Use `v1.2.3-rc.1` for a prerelease and explain its status in the changelog.
3. Run the Release workflow against that tag. It runs the shared preflight and exact minimum-compiler tests before native platform tests and locked release builds.
4. Each build packages, extracts, and runs version plus offline library add/list/remove checks. It also tests the legacy formula's executable move. The publisher requires all 3 archives and their valid basename-only SHA-256 sidecars.
5. The publisher uses the curated changelog section as release notes, flags prereleases, stages a draft, compares uploaded bytes, and publishes only after verification.
6. Publish the same source to crates.io with `cargo publish --locked` after the binary release succeeds. Verify a clean `cargo install skillator --version <version> --locked` into an isolated prefix and run the functional smoke test.
7. Update the CommandZero Homebrew formula from verified public checksums in a separate reviewable PR. Run `brew install`, `brew test`, and the functional check on all supported hosts before merging it.

No registry publication, tag creation, or tap update is performed by local preflight.
GitHub workflow runs retain failure logs and per-target build artifacts for recovery.

## Reruns and partial failures

Never replace published bytes. A rerun accepts an existing release only when every expected asset matches byte-for-byte, and leaves its publication status and metadata unchanged.
New builds are not assumed to reproduce an earlier archive exactly; reuse retained workflow artifacts when resuming.

If upload or verification fails, the new release remains a draft.
Download its existing assets and compare them against the retained artifacts. Upload only missing assets with the original names and bytes, verify the complete matrix, then publish the reviewed draft manually. Never use `--clobber`.

If a draft contains differing bytes, stop and investigate the source revision and build inputs.
If a published release is incomplete or wrong, document the failure and prepare a new version. Keep its original assets intact.

If crates.io publication fails after the GitHub release, leave the release in place and retry the same crate only after checking registry state.
If the tap update fails, keep the last working formula and retry its reviewed update. Do not rewrite product tags to repair a consumer.

## Local package check

Build and exercise the package on its native host before release preparation.
This produces local files only, and does not claim compatibility on a different host.

```sh
rustup run 1.97.1 cargo build --release --locked --target aarch64-apple-darwin
bash scripts/release-package.sh v0.1.0 aarch64-apple-darwin
```

Replace the example tag with the checkout's manifest version.
The v0.1.0 command is a local test only; the existing published release must not be rebuilt or replaced.
