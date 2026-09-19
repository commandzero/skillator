## 1. Configuration and CLI contract

- [ ] 1.1 Add the optional strict main configuration codec and host alias selection. Verify valid maps, duplicate and reserved aliases, unknown fields, malformed destinations, missing configuration, and explicit host subsets with behavior tests.
- [ ] 1.2 Add the public library rsync options and mode-specific report attribution. Verify help, defaults, invalid values, JSON/YAML equivalence, color conflicts, stdout/stderr routing, and stable exit codes through CLI tests.
- [ ] 1.3 Add versioned participant identity and synchronization-state storage outside library configuration. Verify history loss, incompatible state versions, identity replacement, and preservation of old state on write failure.

## 2. Remote protocol and read-only preflight

- [ ] 2.1 Add the private remote entry point and SSH adapter with structured messages, validated destinations, bounded I/O, cancellation, and protocol negotiation. Verify protocol separation from diagnostics and reject malformed or incompatible messages without mutation.
- [ ] 2.2 Implement all-host preflight for reachability, remote home, Skillator compatibility, Git, rsync, and configuration. Verify one unavailable or incompatible endpoint aborts before any writes on every participant and names the failed host.
- [ ] 2.3 Implement read-only observation and physical home-relative path mapping. Verify different home roots, external locations, environment expressions, excluded paths, overlap conflicts, escaping symlinks, missing optional configuration, and unsafe destinations.

## 3. Exact-commit Git sources

- [ ] 3.1 Observe each initiating Git source's origin identity, checked-out commit, source root, working-tree content, and index state. Verify ordinary repositories, worktree and submodule source roots, detached HEAD, dirty tracked skills, and unmerged-index blocking with disposable Git repositories.
- [ ] 3.2 Stage clones from origin at the exact initiating commit and publish only to absent validated destinations. Verify an advanced upstream does not change the selected commit, private or unavailable origins fail clearly, and an unobtainable commit never falls back to a branch head.
- [ ] 3.3 Validate existing checkout identity and commit before source synchronization. Verify mismatched commits, wrong origins, and occupied non-repository destinations remain unchanged on all hosts for that source while independent sources remain eligible.
- [ ] 3.4 Preserve existing Git indexes and unrelated working-tree content during skill synchronization. Verify tracked skill edits transfer as uncommitted changes, unrelated dirty files remain unchanged, remote-only Git sources report the missing initiating reference, and no pull, merge, reset, or commit runs.

## 4. Multi-host file planning

- [ ] 4.1 Build complete skill observations with hashes, file types, executable modes, exclusions, and historical paths. Verify hidden and unselected skills, supporting files, invalid manifests, manifest deletion, unreadable entries, library acquisition links, and unsupported internal links.
- [ ] 4.2 Implement comparisons using shared Git content for first-contact tracked paths and acknowledged per-participant baselines thereafter. Verify one-sided and identical edits, conflicting first-contact untracked files, mode changes, source identity collisions, and lost-history behavior.
- [ ] 4.3 Implement missing copy/remove/ignore and conflict local/remote/ask precedence. Verify default restoration, history-proven removal, first-contact removal refusal, delete-versus-edit conflicts, explicit absence selection, and preservation of unrelated siblings.
- [ ] 4.4 Plan across all selected hosts before publication. Verify host-order permutations produce the same outcomes, host A edits reach host B in the same run, multiple changed remote candidates remain unresolved under remote policy, and unselected hosts remain untouched.
- [ ] 4.5 Implement interactive candidate selection and noninteractive unresolved reporting. Verify skip preserves content, machine formats never prompt, check mode never prompts, and independent files continue when conflicts remain.

## 5. Transfer, publication, and recovery

- [ ] 5.1 Add rsync staging through the initiating host using explicit planned file lists. Verify real transfers preserve complete skill content and executable modes without copying Git administrative files, unrelated files, user symlinks, or target registries.
- [ ] 5.2 Add deterministic locks, verified staging, precondition rechecks, backups, and publication. Verify competing initiators, source changes, destination changes, parent-path replacement, and hash mismatches cannot overwrite intervening edits.
- [ ] 5.3 Persist acknowledgements only for verified participant outcomes and retain recoverable failure state. Inject failures during transfer, publication, verification, and acknowledgement; verify rollback or recovery reports and safe retry after partial completion.
- [ ] 5.4 Implement end-to-end check mode with no persistent local or remote writes. Verify absent clones are reported as bootstrap plus unverified follow-on work and compare participant filesystem snapshots before and after checks and failed global preflight.

## 6. Library and user state

- [ ] 6.1 Register successfully prepared corresponding home-relative Locations while preserving receiving registrations, order, exclusions, and overlap rules. Verify conflicting existing settings block the Location and registration removal does not occur.
- [ ] 6.2 Merge user Skill Directory definitions and Enablements semantically with shared history. Verify first-contact union, explicit deselection under missing copy, concurrent mode edits, coupled directory removal, and missing configuration without mass deselection.
- [ ] 6.3 Reconcile merged user state with local paths through protected prepared mutations. Verify rebuilt links across different homes, copied mode, blocked source dependencies, unmanaged-entry preservation, guarded copied-materialization drift, and separate desired-state/materialization outcomes after failure.

## 7. Integrated acceptance and documentation

- [ ] 7.1 Exercise a three-participant scenario containing central skills, an external Git source pinned behind upstream, tracked and untracked edits, user selections, conflicts, and opted-in removals. Verify same-run propagation, repeat-run idempotence, exact commit preservation, and retry after an interrupted host.
- [ ] 7.2 Verify the real SSH and rsync workflow on supported macOS and Linux environments, including different user homes and dependency failures. Record the environments and evidence; do not represent mocked transport coverage as real host validation.
- [ ] 7.3 Document main host configuration, flags, exact-commit bootstrap, separate update workflow, Git authentication, mismatch remediation, preview limits, and recovery. Verify examples against implemented help and add the user-visible change to the changelog.
- [ ] 7.4 Run the repository's required preflight and strict OpenSpec validation after implementation. Resolve change-scoped failures and record the results before requesting review.
- [ ] 7.5 Before merging an implementation PR, synchronize and archive this change and create the repository-required semantic review receipt. Verify the PR-scoped contract passes and no unrelated change is archived.
