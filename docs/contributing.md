---
type: Playbook
title: Contributing
description: Local validation, pull-request checks, and contribution rules.
status: draft
generated: { by: openai-codex/gpt-6.1-sol, at: 2026-10-07T03:48:45Z }
---

# Contributing

Use one local entry point for the checks required by CI.
Run the commands in this guide from the repository root.

```sh
bash scripts/tools-setup.sh
bash scripts/preflight.sh
```

Setup installs pinned Actionlint, OpenSpec, and OKF under the ignored `.tools` directory.
ShellCheck, ripgrep, Git, and rsync must be on PATH. Rsync must support `--delete-delay`; install a current rsync when an older system copy lacks it. Rustup selects Rust 1.97.1 even when a system Cargo comes first on PATH.

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

Unsafe code is denied by default. The private filesystem module uses atomic rename APIs absent from the standard library. Library delivery uses ordinary SSH and rsync without descriptor-binding or a custom remote server.
The private update-process module uses POSIX signal handlers, process-group signals, and nonblocking pipe flags to bound Git pull subprocesses.
The private remote-process module uses process-group signals to cancel TUI SSH/rsync delivery and to cancel and bound hostname probes and replica inspection, including their descendants. Delivery has no artificial transfer deadline.
Keep CString lifetime, signal ownership, descriptor lifetime, and platform-flag safety explanations next to each unsafe block. Expand this exception only with a documented need and focused behavior tests.

## TUI module boundaries

Keep scope workflows in `src/tui/`:

- `user.rs` owns User loading and directory-tab construction.
- `repo.rs` owns Repo loading, repository-owned skills and tracking exceptions.
- `library.rs` owns Library discovery, acquisition, host browsing and follower delivery.
- `target.rs` shares the User/Repo directory-editing, background-loading and save lifecycle.

`mod.rs` owns terminal restoration, cached scope navigation and event dispatch.
`model.rs`, `reducer.rs`, `input.rs` and `render.rs` share state, transitions, key decoding and rendering.
Scope workflows execute effects; navigation uses typed outcomes rather than reserved exit statuses.
Keep User/Repo differences in their scope modules instead of duplicating the directory event loop.
Cross-scope behavior coverage lives in `src/tui/tests.rs`; public interaction coverage lives in `tests/tui.rs` and `tests/acceptance.rs`.

## Commits and history

Use a Conventional Commit title for every PR and its squash commit.
Allowed types are `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, and `revert`.

Scope is optional. Use `!` and explain the affected contract for a breaking change.
Keep historical commits intact; temporary worktree commits do not need rewriting.

Add user-visible changes to the appropriate Unreleased category in CHANGELOG.md.
Do not add empty categories or require a changelog entry for every internal change.

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

1. Start the TUI in an interactive terminal and open help. Verify root startup selects Repo in Git and User in the physical home directory, including home-as-Git and absent Library config.
2. Resize the terminal, cycle Library/User/Repo with Ctrl+H/Ctrl+L (also from `skillator library`), cycle sub-tabs with Tab, and filter the list. Check gray Library borders, bone titles, editor Backspace and dirty-scope guards.
   With deliberately slow Git discovery, check cached Library entry renders without waiting and cold `skillator library` accepts input while loading. Add a skill externally and verify background updates preserve selected identity/filter/collapse state. Complete a refresh during an editor or staged acquisition, then cancel/undo and check no edits are lost. Replace Library configuration during a scan and confirm obsolete replies are ignored; change filesystem safety after caching and confirm Save revalidates it. Check SSH inspection cancels on scope/host exit and restarts in the retained follower tab.
   Check both exits from Library, including first User/Repo entry after explicit Library launch: cached/loading views and scope controls must not wait for destination observation. While refresh runs, stage checks/modes in one directory, browse another, and open an editor; no state may be overwritten. Insert a configured directory externally and check selected directory/row identity, filters and collapse state survive. Undo and save must discard old replies, reload fresh state and retain live configuration/destination guards.
   Delete the last of two User directories while Repo directories exist: selection and the editor must stay in User, and the deletion must remain staged. Repeat in Repo and verify the one-directory minimum and enabled-skill guards. Warm Target inventory, add/remove/change a skill externally, leave Library before its scan finishes, and confirm destination refresh and Undo see the disk changes. Stage a Library visibility edit, cancel the Target picker and confirm the edit remains; then accept a different Target and return to Library to confirm discarded edits do not reappear and saved files remain unchanged.
3. Use `Ctrl+T` to stage a preset and custom directory; cancel/discard and confirm no skill files or configuration changed. Save and restart to check scope isolation. With a disposable trusted-key SSH follower, verify its differing alias/hostname and read-only replica tab without changing SSH trust or unrelated replica contents. Save a newly registered host, decline initialization, and confirm its registration persists without a replica; repeat with two new hosts to check prompt order and host-only `Ctrl+S` deferred exit. On a saved follower, confirm `s` and `Ctrl+S` offer the owned-replica overwrite/stale-file-deletion boundary and never save Local edits or exit. Decline once, then accept and observe a real leader-to-selected-follower rsync and refreshed inspection; verify Local `s` still saves. During a slow transfer, navigate away and check the UI remains responsive, duplicates do not start another transfer, cancellation stops local SSH/rsync descendants, private exports are cleaned, and stale results do not replace the next host's view. If a transfer is interrupted, check the owned replica for partial content instead of assuming rollback. Inspect failure details and retained export cleanup diagnostics when relevant.
   Deliver a skill whose `SKILL.md` is an internal relative symlink, then inspect its follower tab and document. The link must remain intact and readable; escaping, absolute, broken, cyclic and overlong link chains must fail without replica writes. A supporting directory named `SKILL.md` must not become an inventory document.
4. Quit and check that normal input, echo, cursor visibility, and the alternate screen recover.

Record the host and terminal used. Linux, WSL, and release-target checks require those actual environments.
