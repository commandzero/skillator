## 1. Clone-local Target state

- [x] 1.1 Refactor Target configuration persistence so `.agents/skillator.yaml` is a clone-local file and add the two exact root `.gitignore` rules without rewriting unrelated ignore content or mutating the Git index.
- [x] 1.2 Detect tracked, staged, unmerged, unreadable, and concurrently changed local Target configuration before publication; preserve protected content and report the exact `git rm --cached -- .agents/skillator.yaml` remediation for legacy tracked configuration.
- [x] 1.3 Replace catch-all Skill Directory controls with grouped parent `.gitignore` generation that ignores only configured materializations and Skillator recovery artifacts.
- [x] 1.4 Update observation and reconciliation so unmanaged repository-owned skills are diagnostics only, remain Git-trackable, and never become removal work; retain explicit removal for a Skill the local desired state disables.
- [x] 1.5 Add focused configuration, ignore-rule, control-file, and tracked-entry regression tests, including independently tracked repository-owned skills beside managed entries.

## 2. Shared Target-state publication

- [x] 2.1 Introduce the private Target-state planner that prepares root ignore updates, conditional local configuration publication, grouped control files, and reconciliation from one validated desired configuration.
- [x] 2.2 Reuse existing containment, sibling staging, fingerprint revalidation, lock, rollback, and report boundaries for every Target-state publication; add tests for partial reconciliation after a successful configuration write.
- [x] 2.3 Route TUI saves and ordinary non-interactive sync through the new local-state behavior without allowing ordinary `skillator sync` to create or alter desired configuration.

## 3. Git worktree discovery and projection

- [x] 3.1 Add a narrow Git worktree-discovery API that identifies the canonical primary and current linked-worktree roots from Git metadata and rejects primary, ordinary, and non-Git directories.
- [x] 3.2 Add the `worktree sync` command shape, help text, format handling, and invalid-input reporting while preserving the existing command output and exit-code contract.
- [x] 3.3 Implement worktree projection: read and validate the primary's local configuration, acquire primary and destination locks in stable canonical-path order, revalidate the source fingerprint, and publish only into the current linked worktree.
- [x] 3.4 Classify destination configuration state correctly: absent is Safe, identical is a no-op, differing untracked state is Guarded, and tracked, staged, unmerged, unreadable, or changed state is Blocked; make `--check` write nothing and `--force` cover viable guarded replacements.
- [x] 3.5 Reconcile the destination using its copied configuration and the current user's Library, reporting unresolved Skills and partial convergence through the normal report path.
- [x] 3.6 Add fixture-based tests for successful linked-worktree projection, invalid invocation locations, missing or invalid primary configuration, differing destination configuration, force/check behavior, source change races, and one unavailable Source.

## 4. User-facing documentation and verification

- [x] 4.1 Update README and in-product remediation text to explain clone-local Target configuration, repository-owned skills, selective local controls, and `skillator worktree sync`.
- [x] 4.2 Update the relevant main OpenSpec specifications after implementation, then run the complete test suite, formatting, Clippy with warnings denied, strict OpenSpec validation, and Linux validation on `ironhide.local`.
