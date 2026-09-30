---
type: Playbook
title: Contributing
description: Local validation, pull-request checks, and contribution rules.
status: draft
generated: { by: codex/gpt-6, at: 2026-09-07T06:07:29Z }
---

# Contributing

Use one local entry point for the checks required by CI.
Run the commands in this guide from the repository root.

```sh
bash scripts/tools-setup.sh
bash scripts/preflight.sh
```

Setup installs pinned Actionlint, OpenSpec, and OKF under the ignored `.tools` directory.
ShellCheck and ripgrep must be on PATH. Rustup selects Rust 1.97.1 even when a system Cargo comes first on PATH.

The full preflight runs formatting, strict Clippy, locked behavior tests, library doctests, shell and workflow checks, documentation-bundle validation, main-spec validation, and repository-tool tests.
Preflight runs Rust tests serially within each test binary. Parallel tests that write executable fixtures and spawn subprocesses can briefly retain writable descriptors across a fork on Linux, causing `Text file busy` when another test executes a new hook. CI platform jobs still run in parallel.
There are no optional Cargo features, so default and minimal feature sets are identical.

Use `bash scripts/preflight.sh test` for another supported host or compiler.
Use `bash scripts/preflight.sh policy` for documentation or workflow work that does not change product code.

```sh
rustup toolchain install 1.97.0 --profile minimal
RUST_TOOLCHAIN=1.97.0 bash scripts/preflight.sh test
```

## Scope and source safety

This repository follows the selected CommandZero standards.
In the team workspace, AGENTS.md supplies the local pointer to the repo-man standards bundle.

The repository rules in this document make the adopted validation contract explicit.
Changes to broader draft standards do not silently change this contract.

Keep the product as one crate until a real consumer or dependency boundary justifies a split.
The repository checker is a Cargo example using existing development dependencies; it adds no installed command.

Unsafe code is denied by default. The private filesystem module uses atomic rename APIs and descriptor-relative directory operations absent from the standard library. Remote transport binds rsync subprocesses to verified directory descriptors with `fchdir`, so replacing a staging path cannot redirect a transfer.
The private update-process module uses POSIX signal handlers, process-group signals, and nonblocking pipe flags to bound Git pull subprocesses.
Keep CString lifetime, signal ownership, descriptor lifetime, and platform-flag safety explanations next to each unsafe block. Expand this exception only with a documented need and focused behavior tests.

## Commits and history

Use a Conventional Commit title for every PR and its squash commit.
Allowed types are `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, and `revert`.

Scope is optional. Use `!` and explain the affected contract for a breaking change.
Keep historical commits intact; temporary worktree commits do not need rewriting.

Add user-visible changes to the appropriate Unreleased category in CHANGELOG.md.
Do not add empty categories or require a changelog entry for every internal change.

## Rust API migration

The unreleased removal of `reconcile::prepare_transition_with_locks` is a source-breaking Rust API change and must ship in 0.2.0, not a 0.1.x patch.
Call `reconcile::prepare_transition_with_locks_and_repository_skills(target, original, staged, library, &RepositorySkillExceptions::new(), locks)` instead, importing `RepositorySkillExceptions` from `skillator::target`.
This preserves caller-owned lock transfer while explicitly supplying an empty repository-skill exception set. Callers that do not already hold locks can continue using `reconcile::prepare_transition`.
The CLI reconciliation commands are unchanged by this API removal.

## OpenSpec completion

Every PR must contain one association field with comma-separated change IDs.
Implementation work must name its change even when the artifacts were committed earlier.

```text
OpenSpec changes: add-example
```

For work without an OpenSpec change, state that fact and explain it.

```text
OpenSpec changes: none
OpenSpec reason: Contributor tooling only; no product behavior changes.
```

The gate compares committed PR state with its merge base. It selects edited, deleted, renamed, active and newly archived change paths, plus explicit associations.
Unrelated active changes do not block the PR. Deleting an active directory without an archive fails.

For each associated change:

1. Synchronize its additions, modifications, removals, and renames into the main specs, then archive it. Keep the proposal, design, tasks, and delta specs.
2. Commit that state. Review each affected requirement and scenario against the main specs. For renamed specs, inspect both the old path and the destination.
3. Write the semantic findings to a local notes file. Explain any deliberately absent spec or change with no deltas.
4. Record the review with the command below, then commit the generated receipt. Use `--no-spec-deltas` only when the review explains why no deltas exist. Append renamed destination spec paths when they do not appear in the delta folder names.

```sh
cargo run --locked --example repo-check -- review --reviewed \
  add-example reviewer-name /tmp/example-review.md \
  openspec/specs/renamed-capability/spec.md
```

The receipt binds the review to the committed archive and spec Git objects, including absent removed specs.
The gate rejects missing receipts and later changes to reviewed content. It validates main specs and only the selected archives with pinned OpenSpec.

The receipt is an explicit review record, not automated proof of semantic equivalence or reviewer identity.
The PR reviewer must check its findings before approving. Already-synchronized specs can pass without another spec diff.

Run the PR contract locally with the same inputs as CI:

```sh
PR_BASE=origin/main PR_HEAD=HEAD \
PR_TITLE='ci: enforce repository checks' \
PR_BODY_FILE=/tmp/pr-body.md bash scripts/preflight.sh pr
```

## Required GitHub checks

Require `preflight`, `pr-contract`, and both `Test ... Rust ...` jobs before merge.
Require the branch to be current with the target branch, and require review of changes to validation code and synchronization receipts.

CI reruns on PR head, title, body, and base edits. Updating the PR branch after a target-branch change recomputes the merge base.
Repository administrators must enforce these rules in GitHub; workflow files alone cannot establish branch protection.

## Terminal checks

Automated TUI tests run in preflight. For changes to terminal interaction, also use an isolated HOME and a temporary Git repository.

1. Start the TUI in an interactive terminal and open help.
2. Resize the terminal, move between Library and Skills, and filter the list.
3. Preview a change, cancel it, and confirm that no skill files changed.
4. Quit and check that normal input, echo, cursor visibility, and the alternate screen recover.

Record the host and terminal used. Linux, WSL, and release-target checks require those actual environments.
