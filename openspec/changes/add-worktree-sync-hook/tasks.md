## 1. Git hook discovery and report model

- [ ] 1.1 Extend Git discovery to resolve the effective hooks path, common Git directory, and linked-worktree state, and verify it with repository fixtures covering the main worktree, a linked worktree, a clone, and a configured `core.hooksPath`.
- [ ] 1.2 Define hook ownership states, fingerprints, change outcomes, and compact report types, then verify deterministic text, JSON, and YAML rendering with unit tests.

## 2. Hook file lifecycle

- [ ] 2.1 Implement the generated POSIX `post-checkout` hook template with a stable Skillator marker, null-ref and linked-worktree checks, Git environment sanitization, the `SKILLATOR_NO_AUTO_SYNC` opt-out, and advisory `skillator sync worktree .` execution; verify the rendered script has the expected executable permissions and arguments.
- [ ] 2.2 Implement atomic hook installation with check mode, idempotency, stale-write fingerprints, and safe handling of missing, modified, unreadable, non-regular, and conflicting hook paths; verify no-write previews and failed-precondition behavior.
- [ ] 2.3 Implement guarded chaining and uninstall, preserving an existing hook's bytes and permissions, forwarding its exit status, restoring it only when managed files are unchanged, and refusing ambiguous or modified removal; verify chain order and ownership protections.

## 3. CLI integration

- [ ] 3.1 Add `skillator hook install [repository]`, `skillator hook status [repository]`, and `skillator hook uninstall [repository]` with the existing output, `--check`, `--force`, color, and terminal-independent parsing rules; verify help, defaults, option conflicts, and non-TTY execution.
- [ ] 3.2 Dispatch hook workflows without touching Target, User Scope, Library, index, or tracked files, and return the established statuses for success, guarded conflict, invalid Git input, blocked ownership, and fatal I/O; verify each report's affected path and exit code.

## 4. Event and synchronization integration tests

- [ ] 4.1 Exercise a real `git worktree add` with the installed hook and verify the new worktree receives the same result as an explicit `skillator sync worktree` invocation.
- [ ] 4.2 Verify the hook skips ordinary branch and file checkouts, the primary worktree after `git clone`, and `git worktree add --no-checkout`, while the explicit sync command still works for the latter case.
- [ ] 4.3 Verify missing Skillator binaries, unavailable primary configuration, unresolved Skills, guarded sync results, and `SKILLATOR_NO_AUTO_SYNC=1` produce diagnostics without making the created worktree unavailable.
- [ ] 4.4 Verify concurrent installation or removal cannot replace a hook changed after planning, and run the full Rust test suite for the new integration paths.

## 5. Documentation and agent workflow

- [ ] 5.1 Update CLI help and README documentation with hook installation, status, uninstall, opt-out, explicit retry, and the `--no-checkout` limitation; verify examples match `skillator --help`.
- [ ] 5.2 Update the project-owned agent skill to retain explicit `git worktree add` plus `skillator sync worktree` as the portable agent and CI workflow, while documenting the hook as optional convenience; verify the skill's lifecycle guidance remains consistent with the installed CLI.

## 6. Validation

- [ ] 6.1 Run `cargo fmt --check`, `cargo test --all-targets`, and `cargo clippy --all-targets -- -D warnings`, then validate the OpenSpec change with `OPENSPEC_TELEMETRY=0 openspec validate --change add-worktree-sync-hook --strict`.
