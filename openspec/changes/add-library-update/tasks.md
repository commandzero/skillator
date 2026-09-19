## 1. Discovery and planning

- [ ] 1.1 Diagnose incomplete discovery and select canonical Git roots. Verify unavailable Locations, unreadable subtrees, exclusions, symlinks, overlaps, colliding keys, empty repositories, nested repositories, and worktrees.
- [ ] 1.2 Skip initialized submodules including direct Locations. Verify advisory-only results without pull rows or failure solely for skips.

## 2. Git execution

- [ ] 2.1 Add read-only eligibility and identity checks. Verify dirty, ignored, detached, upstream, operation-state, and changed-root cases.
- [ ] 2.2 Implement configured-upstream fast-forward-only pulls overriding conflicting settings. Verify current, ahead, behind, divergent, non-origin upstream, partial failure, and retained success against local remotes.
- [ ] 2.3 Capture output and disable prompts. Verify authentication failure cannot contaminate machine output or prompt.
- [ ] 2.4 Add default 30-second configurable deadlines and process-group cleanup. Verify stalled descendants stop, pipes do not hang, failures continue, and Ctrl+C yields a partial report with exit 130.

## 3. CLI and reports

- [ ] 3.1 Add command parsing and dispatch. Verify non-TTY use outside Git, options, default timeout, invalid timeouts, selectors, and force rejection.
- [ ] 3.2 Implement local-only preview with exact wording and machine advisory. Verify exit semantics, no network, and unchanged Git metadata and files.
- [ ] 3.3 Render deterministic equivalent JSON and YAML and concise text. Verify stable diagnostics, partial results, skipped submodules, and pre-report errors.
- [ ] 3.4 Preserve declarations and copies. Verify existing links expose Source changes while configuration, registries, control files, and copies remain unchanged.

## 4. Documentation and validation

- [ ] 4.1 Update help and workflow documentation for submodules, timeout, preview, cancellation, partial failure, and copy synchronization. Verify examples and links.
- [ ] 4.2 Run relevant integration tests and required repository preflight; record results and resolve introduced regressions.
- [ ] 4.3 Validate the change and review acceptance scenarios against implementation before any later synchronization or archival.
